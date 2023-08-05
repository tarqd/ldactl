use std::{
    borrow::BorrowMut,
    ops::{Add, AddAssign, Deref, DerefMut},
    pin::{self, pin, Pin},
    task::Poll::{self, Pending, Ready},
    time::Duration,
};

use tokio_sse_codec::{self as sse_codec, Event};

use super::sse_backoff::{MinimumBackoffDuration, WithMinimumBackoff};
use crate::eventsource::{
    errorext::EventSourceErrorInnerError,
    retryable::Retryable,
    state_util::{macros::run_state, EventSourceState, NextState, StateAction, StateProj},
};

use backoff::{backoff::Backoff, retry, ExponentialBackoff};
use futures::{Future, FutureExt, StreamExt, TryStreamExt};

use miette::Diagnostic;
use pin_project::pin_project;
use reqwest::{RequestBuilder, Response};
use thiserror::Error;
use tokio_stream::Stream;

use tokio_util::{codec::FramedRead, compat::FuturesAsyncReadCompatExt};
use tracing::{debug, debug_span, error, error_span, instrument, trace, warn, Span};
use tracing_futures::Instrument;

#[derive(Debug, Error, Diagnostic)]
pub enum EventSourceError {
    #[error("request builder must be cloneable to retry")]
    #[diagnostic(help("make sure the request builder doesn't use streams or other non-cloneable types in the body"))]
    RequestCloneError,
    #[error("request error")]
    RequestError(#[from] reqwest::Error),
    #[error("max retries exceeded after {0} attempts")]
    #[help = "you can tune max retries by customizing the backoff strategy passed to the event source"]
    MaxRetriesExceeded(usize, #[source] Option<Box<EventSourceError>>),
    #[error("error while decoding sse event")]
    #[diagnostic(help("set RUST_LOG=\"{}::eventsource::sse_codec=debug\"", env!("CARGO_PKG_NAME")))]
    DecodeError(#[from] sse_codec::DecodeError),
    #[error("read timed out after {1:?}")]
    ReadTimeoutElapsed(#[source] tokio_stream::Elapsed, Duration),
    #[error("io error")]
    Io(#[from] std::io::Error),
}

#[pin_project]
pub struct EventSource {
    request_builder: RequestBuilder,
    backoff: MinimumBackoffDuration<Box<dyn Backoff>>,
    #[pin]
    state: EventSourceState,
    retry_attempts: usize,
    last_event_id: Option<String>,
    read_timeout: Duration,
}

impl EventSource {
    #[instrument(skip(req, backoff))]
    pub fn try_with_backoff<T>(
        req: RequestBuilder,
        last_event_id: Option<String>,
        backoff: T,
    ) -> Result<Self, EventSourceError>
    where
        T: Backoff + Sized + 'static,
    {
        let builder = req
            .header("accept", "text/event-stream")
            .try_clone()
            .ok_or_else(|| EventSourceError::RequestCloneError)?;

        let b: Box<dyn Backoff> = Box::new(backoff);

        Ok(Self {
            request_builder: builder,
            backoff: b.with_minimum_duration(Duration::ZERO),
            state: EventSourceState::New(debug_span!(parent:None, "new").entered()).into(),
            retry_attempts: 0,
            last_event_id: last_event_id,
            read_timeout: Duration::from_secs(5 * 60),
        })
    }
    #[instrument(skip(self,parent),fields(last_event_id=?self.last_event_id, attempt=self.retry_attempts+1))]
    fn send_request(self: Pin<&mut Self>, parent: Option<tracing::Id>) -> (StateAction, NextState) {
        Span::current().follows_from(parent);
        debug!("opening connection to event source");
        let mut builder = match self.request_builder.try_clone() {
            Some(builder) => {
                debug!("starting new request to event source");
                builder
            }
            None => {
                error!("request builder must be cloneable to retry");

                return (
                    StateAction::Break(Ready(Some(
                        Err(EventSourceError::RequestCloneError.into()),
                    ))),
                    Some(EventSourceState::Closed),
                );
            }
        };
        if let Some(last_event_id) = &self.last_event_id {
            trace!("setting last-event-id header to {}", last_event_id);
            builder = builder.header("last-event-id", last_event_id.clone());
        }

        return (
            StateAction::Continue,
            Some(EventSourceState::Connect(
                builder.send().in_current_span().boxed(),
                debug_span!(parent: None, "send_request", attempt=self.retry_attempts+1).entered(),
            )),
        );
    }

    #[instrument(parent=&parent, skip(self,response, parent), fields(host=response.url().host_str(), path=response.url().path()))]
    fn open_stream(
        self: Pin<&mut Self>,
        response: Response,
        parent: tracing::span::EnteredSpan,
    ) -> (StateAction, NextState) {
        debug!("connected to event source");

        let read_timeout = self.read_timeout.clone();
        let last_event_id = self.last_event_id.clone();

        let inner = tokio_stream::StreamExt::timeout(response.bytes_stream(), read_timeout)
            .map(move |v| match v {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(e)) => Err(EventSourceError::RequestError(e)),
                Err(e) => Err(EventSourceError::ReadTimeoutElapsed(e, read_timeout)),
            })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            .into_async_read()
            .compat();

        let framed_read = FramedRead::new(inner, sse_codec::Decoder::new(last_event_id, None))
            .map_err(|e| EventSourceError::DecodeError(e))
            .in_current_span()
            .boxed();

        (
            StateAction::Continue,
            Some(EventSourceState::Connected(
                framed_read,
                debug_span!("connected").entered(),
            )),
        )
    }

    #[instrument(skip(self,e), fields(attempt=self.retry_attempts+1, error=%e))]
    fn handle_error(
        mut self: Pin<&mut Self>,
        e: impl EventSourceErrorInnerError + 'static,
    ) -> (StateAction, NextState) {
        let e = e.into_event_source_error();
        self.as_mut().project().retry_attempts.add_assign(1);
        let retry_attempts = self.retry_attempts;
        //let span = error_span!("handle_error").entered();

        if e.is_retryable() {
            if let Some(retry_duration) = self.as_mut().project().backoff.next_backoff() {
                debug!(next_attempt=?retry_duration, "recoverable error occurred, will retry");
                (
                    StateAction::Continue,
                    Some(EventSourceState::WaitingForRetry(
                        tokio::time::sleep(retry_duration),
                        Span::current().entered(),
                    )),
                )
            } else {
                // too many attempts
                error!(error=%e, "recoverable error occured, max retries exceeded, closing event source");
                (
                    StateAction::Break(Ready(Some(Err(EventSourceError::MaxRetriesExceeded(
                        retry_attempts,
                        Some(Box::new(e)),
                    ))))),
                    Some(EventSourceState::Closed),
                )
            }
        } else {
            error!(error=%e, "unrecoverable error occured, closing event source");
            (
                StateAction::Break(Ready(Some(Err(e)))),
                Some(EventSourceState::Closed),
            )
        }
    }
}

impl TryFrom<RequestBuilder> for EventSource {
    type Error = EventSourceError;

    fn try_from(req: RequestBuilder) -> Result<Self, Self::Error> {
        Self::try_with_backoff(req, None, ExponentialBackoff::default())
    }
}

impl Stream for EventSource {
    type Item = Result<Event, EventSourceError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        loop {
            let this = self.as_mut().project();
            let state = this.state.project();
            #[allow(unreachable_code)]
            break match state {
                StateProj::New(follow) => {
                    run_state!(self, send_request(None))
                }

                StateProj::Connect(req, parent) => {
                    let p = &*parent;
                    let span = debug_span!(parent: p, "connect").entered();

                    match futures::ready!(req
                        .poll_unpin(cx)
                        .map(|r| r.and_then(Response::error_for_status)))
                    {
                        Ok(response) => {
                            *self.as_mut().project().retry_attempts = 0;
                            self.as_mut().project().backoff.reset();
                            run_state!(self, open_stream(response, span))
                        }
                        Err(e) => run_state!(self, handle_error(e)),
                    }
                }
                StateProj::Connected(stream, parent) => {
                    use sse_codec::Frame;

                    let span = debug_span!(parent:&*parent, "read_frame").entered();

                    break match futures::ready!(stream.poll_next_unpin(cx)) {
                        Some(Ok(frame)) => match frame {
                            Frame::Comment(comment) => {
                                let _span = debug_span!("read_frame::comment", ?comment).entered();
                                span.record("kind", "comment");
                                debug!(comment, "received comment");

                                continue;
                            }
                            Frame::Event(event) => {
                                let _span =
                                    debug_span!("read_frame::event", name=event.name, id=?event.id, data_len=event.data.len())
                                        .entered();
                                debug!("received event");
                                if event.id.is_some() && event.id != *this.last_event_id {
                                    *this.last_event_id = event.id.clone()
                                }

                                Ready(Some(Ok(event)))
                            }
                            Frame::Retry(duration) => {
                                let _span = debug_span!("read_frame::retry", ?duration).entered();
                                debug!("received retry field, updated minimum duration");

                                self.as_mut()
                                    .project()
                                    .backoff
                                    .set_minimum_duration(duration);
                                continue;
                            }
                        },
                        Some(Err(e)) => run_state!(self, handle_error(e)),
                        None => Poll::Ready(None),
                    };
                }
                StateProj::WaitingForRetry(mut sleep, parent) => {
                    let span = debug_span!(parent: &*parent, "retry::wait").entered();
                    match futures::ready!(sleep.poll_unpin(cx)) {
                        () => {
                            self.as_mut()
                                .project()
                                .state
                                .set(EventSourceState::New(span));
                            continue;
                        }
                    }
                }
                StateProj::Closed => break Ready(None),
            };
        }
    }
}
