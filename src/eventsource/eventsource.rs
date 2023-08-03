use std::{error::Error, task::Poll::Ready, time::Duration};

use crate::eventsource::sse_codec::{self, Event};

use super::sse_backoff::MinimumBackoffDuration;
use crate::eventsource::retryable::Retryable;
use backoff::{backoff::Backoff, ExponentialBackoff};
use futures::{FutureExt, StreamExt, TryStreamExt};
//use futures::{Future, FutureExt, Stream};
use miette::Diagnostic;
use pin_project::pin_project;
use reqwest::RequestBuilder;
use thiserror::Error;
use tokio_stream::Stream;

use tokio_util::{codec::FramedRead, compat::FuturesAsyncReadCompatExt};
use tracing::{
    debug, debug_span, error, error_span, info, info_span, instrument, trace, trace_span, warn,
    warn_span,
};

#[derive(Debug, Error, Diagnostic)]
pub enum EventSourceError {
    #[error("request builder must be cloneable to retry")]
    #[diagnostic(help("make sure the request builder doesn't use streams or other non-cloneable types in the body"))]
    RequestCloneError,
    #[error("request error: {0}")]
    RequestError(#[from] reqwest::Error),
    #[error("max retries exceeded after {0} attempts")]
    #[help = "you can tune max retries by customizing the backoff strategy passed to the event source"]
    MaxRetriesExceeded(usize),
    #[error("error while decoding sse event")]
    #[diagnostic(help("set RUST_LOG=\"{}::eventsource::sse_codec=debug\"", env!("CARGO_PKG_NAME")))]
    DecodeError(#[from] sse_codec::SSEDecodeError),
    #[error("read timed out after {1:?}")]
    ReadTimeoutElapsed(#[source] tokio_stream::Elapsed, Duration),
}

#[pin_project]
pub struct EventSource<T>
where
    T: Backoff + Sized,
{
    request_builder: RequestBuilder,
    backoff: MinimumBackoffDuration<T>,
    #[pin]
    state: RetryRequestState,
    past_retries: usize,
    last_event_id: Option<String>,
    read_timeout: Duration,
}

impl<T> EventSource<T>
where
    T: Backoff + Sized,
{
    #[instrument(skip(req, backoff))]
    pub fn try_with_backoff(
        req: RequestBuilder,
        last_event_id: Option<String>,
        backoff: T,
    ) -> Result<Self, EventSourceError> {
        let builder = req
            .header("accept", "text/event-stream")
            .try_clone()
            .ok_or_else(|| EventSourceError::RequestCloneError)?;

        Ok(Self {
            request_builder: builder,
            backoff: MinimumBackoffDuration::new(backoff, Duration::from_secs(0)),
            state: RetryRequestState::New,
            past_retries: 0,
            last_event_id: last_event_id,
            read_timeout: Duration::from_secs(5 * 60),
        })
    }
}

impl TryFrom<RequestBuilder> for EventSource<ExponentialBackoff> {
    type Error = EventSourceError;

    fn try_from(req: RequestBuilder) -> Result<Self, Self::Error> {
        Self::try_with_backoff(req, None, ExponentialBackoff::default())
    }
}

#[pin_project(project = RetryRequestStateProj)]
enum RetryRequestState {
    New,
    Connect(
        std::pin::Pin<
            Box<dyn futures::Future<Output = Result<reqwest::Response, reqwest::Error>> + Send>,
        >,
    ),
    Connected(
        std::pin::Pin<Box<dyn Stream<Item = Result<sse_codec::Item, EventSourceError>> + Send>>,
    ),
    Retrying,
    WaitingForRetry(#[pin] tokio::time::Sleep),
    Closed,
}

impl<T> Stream for EventSource<T>
where
    T: Backoff + Sized,
{
    type Item = Result<Event, EventSourceError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            let this = self.as_mut().project();
            let state = this.state.project();
            #[allow(unreachable_code)]
            break match state {
                RetryRequestStateProj::New => {
                    let span = debug_span!("new");
                    let _enter = span.enter();
                    let mut builder = match this.request_builder.try_clone() {
                        Some(builder) => {
                            debug!("starting new request to event source");
                            builder
                        }
                        None => {
                            error!("request builder must be cloneable to retry");
                            self.as_mut().project().state.set(RetryRequestState::Closed);
                            break Ready(Some(Err(EventSourceError::RequestCloneError.into())));
                        }
                    };
                    if let Some(last_event_id) = this.last_event_id {
                        debug!("setting last-event-id header to {}", last_event_id);
                        builder = builder.header("last-event-id", last_event_id.clone());
                    }
                    self.as_mut()
                        .project()
                        .state
                        .set(RetryRequestState::Connect(builder.send().boxed()));
                    continue;
                }
                RetryRequestStateProj::Connect(req) => {
                    let span = debug_span!("connect");
                    let _enter = span.enter();
                    let read_timeout = *this.read_timeout;
                    match futures::ready!(req.poll_unpin(cx)) {
                        Ok(resp) => {
                            debug!("connected to event source");

                            let framed_read = tokio_stream::StreamExt::timeout(
                                FramedRead::new(
                                    resp.bytes_stream()
                                        .map_err(|e| {
                                            std::io::Error::new(std::io::ErrorKind::Other, e)
                                        })
                                        .into_stream()
                                        .into_async_read()
                                        .compat(),
                                    sse_codec::SSECodec::new(),
                                )
                                .map_err(|e| EventSourceError::DecodeError(e))
                                .into_stream(),
                                read_timeout.clone(),
                            )
                            .map(move |v| match v {
                                Ok(v) => v,
                                Err(e) => {
                                    Err(EventSourceError::ReadTimeoutElapsed(e, read_timeout))
                                }
                            })
                            .boxed();
                            self.as_mut()
                                .project()
                                .state
                                .set(RetryRequestState::Connected(framed_read));
                            self.as_mut().project().backoff.reset();
                            continue;
                        }
                        Err(e) => {
                            if e.is_retryable() {
                                debug!(error = %e, "recoverable error connecting to event source, will retry");
                                self.as_mut()
                                    .project()
                                    .state
                                    .set(RetryRequestState::Retrying);
                                continue;
                            } else {
                                error!(error = %e, "unrecoverable error connecting to event source");
                                self.as_mut().project().state.set(RetryRequestState::Closed);
                                break Ready(Some(Err(EventSourceError::RequestError(e))));
                            }
                        }
                    }
                }
                RetryRequestStateProj::Connected(stream) => {
                    let span = debug_span!("framed_read");
                    let _enter = span.enter();
                    match futures::ready!(stream.poll_next_unpin(cx)) {
                        Some(Ok(frame)) => match frame {
                            sse_codec::Item::Event(event) => {
                                debug!(event=?event, "received event");
                                if self.last_event_id != event.id {
                                    *self.as_mut().project().last_event_id = event.id.clone();
                                }
                                break Ready(Some(Ok(event)));
                            }
                            sse_codec::Item::Comment(comment) => {
                                debug!(comment=?comment, "received comment");
                                continue;
                            }
                            sse_codec::Item::Retry(retry) => {
                                tracing::trace!(
                                    retry = ?retry,
                                    "received retry: {}ms",
                                    retry.as_millis(),
                                );
                                self.as_mut().project().backoff.set_minimum_duration(retry);
                                continue;
                            }
                        },
                        Some(Err(e)) => {
                            if e.is_retryable() {
                                debug!(error = %e, "recoverable error reading from event source, will retry");
                                self.as_mut()
                                    .project()
                                    .state
                                    .set(RetryRequestState::Retrying);
                                continue;
                            } else {
                                error!(error = %e, "unrecoverable error reading from event source");
                                self.as_mut().project().state.set(RetryRequestState::Closed);
                                break Ready(Some(Err(e)));
                            }
                        }
                        None => break Ready(None),
                    }
                }
                RetryRequestStateProj::Retrying => {
                    let span = debug_span!("retry");
                    let _enter = span.enter();

                    match this.backoff.next_backoff() {
                        Some(duration) => {
                            debug!(duration=?duration, "waiting before retrying");
                            self.as_mut()
                                .project()
                                .state
                                .set(RetryRequestState::WaitingForRetry(tokio::time::sleep(
                                    duration,
                                )));
                            continue;
                        }
                        None => {
                            error!("max retries exceeded");
                            break Ready(Some(Err(EventSourceError::MaxRetriesExceeded(
                                *this.past_retries,
                            ))));
                        }
                    }
                }
                RetryRequestStateProj::WaitingForRetry(mut sleep) => {
                    match futures::ready!(sleep.poll_unpin(cx)) {
                        () => {
                            let span = debug_span!("retry");
                            let _enter = span.enter();
                            debug!("retrying connecting");
                            self.as_mut().project().state.set(RetryRequestState::New);
                            continue;
                        }
                    }
                }
                RetryRequestStateProj::Closed => {
                    let span = debug_span!("closed");
                    let _enter = span.enter();
                    debug!("event source closed");
                    break Ready(None);
                }
            };
        }
    }
}

impl Retryable for EventSourceError {
    fn is_retryable(&self) -> bool {
        match self {
            EventSourceError::RequestCloneError => false,
            EventSourceError::RequestError(e) => e.is_retryable(),
            EventSourceError::MaxRetriesExceeded(_) => false,
            EventSourceError::DecodeError(_) => true,
            EventSourceError::ReadTimeoutElapsed(..) => true,
        }
    }
}
