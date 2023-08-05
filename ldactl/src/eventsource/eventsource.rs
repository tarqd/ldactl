use std::{error::Error, task::Poll::Ready, time::Duration};

use tokio_sse_codec::{self as sse_codec, Event};

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
    warn_span, Instrument,
};

#[derive(Debug, Error, Diagnostic)]
pub enum EventSourceError {
    #[error("request builder must be cloneable to retry")]
    #[diagnostic(help("make sure the request builder doesn't use streams or other non-cloneable types in the body"))]
    RequestCloneError,
    #[error("request error")]
    RequestError(#[from] reqwest::Error),
    #[error("max retries exceeded after {0} attempts")]
    #[help = "you can tune max retries by customizing the backoff strategy passed to the event source"]
    MaxRetriesExceeded(usize),
    #[error("error while decoding sse event")]
    #[diagnostic(help("set RUST_LOG=\"{}::eventsource::sse_codec=debug\"", env!("CARGO_PKG_NAME")))]
    DecodeError(#[from] sse_codec::DecodeError),
    #[error("read timed out after {1:?}")]
    ReadTimeoutElapsed(#[source] tokio_stream::Elapsed, Duration),
    #[error("io error")]
    Io(#[from] std::io::Error),
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
            state: RetryRequestState::New(None),
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
    New(Option<tracing::Span>),
    Connect(
        std::pin::Pin<
            Box<dyn futures::Future<Output = Result<reqwest::Response, reqwest::Error>> + Send>,
        >,
        tracing::Span,
    ),
    Connected(
        std::pin::Pin<Box<dyn Stream<Item = Result<sse_codec::Frame, EventSourceError>> + Send>>,
        tracing::Span,
    ),
    Retrying(tracing::Span),
    RequestError(Option<reqwest::Error>, tracing::Span),
    WaitingForRetry(#[pin] tokio::time::Sleep, tracing::Span),
    Closed,
}

impl<T> Stream for EventSource<T>
where
    T: Backoff + Sized,
{
    type Item = Result<Event, EventSourceError>;
    #[instrument(skip(self, cx))]
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            let this = self.as_mut().project();
            let state = this.state.project();
            #[allow(unreachable_code)]
            break match state {
                RetryRequestStateProj::New(parent) => {
                    let span = debug_span!("new");
                    if let Some(parent) = parent {
                        span.follows_from(parent.clone());
                    }
                    let follow_span = span.clone();
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
                        .set(RetryRequestState::Connect(
                            builder.send().boxed(),
                            follow_span.clone(),
                        ));
                    continue;
                }
                RetryRequestStateProj::Connect(req, parent_span) => {
                    let span = debug_span!(parent: parent_span.clone(), "connect");
                    let follow_span = span.clone();
                    let _enter = span.enter();
                    let read_timeout = *this.read_timeout;
                    match futures::ready!(req.poll_unpin(cx)) {
                        Ok(resp) => match resp.error_for_status() {
                            Ok(resp) => {
                                let inner = tokio_stream::StreamExt::timeout(
                                    resp.bytes_stream(),
                                    this.read_timeout.clone(),
                                )
                                .map(move |v| match v {
                                    Ok(Ok(v)) => Ok(v),
                                    Ok(Err(e)) => Err(EventSourceError::RequestError(e)),
                                    Err(e) => {
                                        Err(EventSourceError::ReadTimeoutElapsed(e, read_timeout))
                                    }
                                })
                                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
                                .into_async_read()
                                .compat();

                                let framed_read = FramedRead::new(
                                    inner,
                                    sse_codec::Decoder::new(self.last_event_id.clone(), None),
                                )
                                .map_err(|e| EventSourceError::DecodeError(e))
                                .boxed();

                                self.as_mut()
                                    .project()
                                    .state
                                    .set(RetryRequestState::Connected(framed_read, follow_span));
                                self.as_mut().project().backoff.reset();
                                continue;
                            }
                            Err(e) => {
                                self.as_mut()
                                    .project()
                                    .state
                                    .set(RetryRequestState::RequestError(
                                        Some(e),
                                        follow_span.clone(),
                                    ));
                                continue;
                            }
                        },
                        Err(e) => {
                            self.as_mut()
                                .project()
                                .state
                                .set(RetryRequestState::RequestError(
                                    Some(e),
                                    follow_span.clone(),
                                ));
                            continue;
                        }
                    }
                }
                RetryRequestStateProj::Connected(stream, parent_span) => {
                    let span = debug_span!(parent: parent_span.clone(), "read_frame");
                    let _entered = span.enter();

                    match futures::ready!(stream.poll_next_unpin(cx)) {
                        Some(Ok(frame)) => match frame {
                            sse_codec::Frame::Event(event) => {
                                debug!(event=?event, "received event");
                                if self.last_event_id != event.id {
                                    *self.as_mut().project().last_event_id = event.id.clone();
                                }
                                break Ready(Some(Ok(event)));
                            }
                            sse_codec::Frame::Comment(comment) => {
                                debug!(comment=?comment, "received comment");
                                continue;
                            }
                            sse_codec::Frame::Retry(retry) => {
                                tracing::trace!(
                                    retry = ?retry,
                                    "received retry: {}ms",
                                    retry.as_millis(),
                                );
                                self.as_mut().project().backoff.set_minimum_duration(retry);
                                continue;
                            }
                        },
                        Some(Err(source_error)) => {
                            let span = debug_span!("read_frame::error");
                            let child_span = span.clone();
                            let _enter = span.enter();
                            // Original error gets wrapped in std::io::Error
                            // because asyncread demands it.
                            // Here we try and get the original error back out.
                            let e = match source_error {
                                EventSourceError::DecodeError(sse_codec::DecodeError::Io(
                                    io_err,
                                )) if io_err.kind() == std::io::ErrorKind::Other
                                    && io_err
                                        .get_ref()
                                        .map(|e| e.downcast_ref::<EventSourceError>())
                                        .flatten()
                                        .is_some() =>
                                {
                                    *io_err
                                        .into_inner()
                                        .unwrap()
                                        .downcast::<EventSourceError>()
                                        .unwrap()
                                }
                                _ => source_error,
                            };

                            if e.is_retryable() {
                                debug!(error = %e, "recoverable error reading from event source, will retry");
                                self.as_mut()
                                    .project()
                                    .state
                                    .set(RetryRequestState::Retrying(child_span));
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
                RetryRequestStateProj::RequestError(opt, parent) => {
                    let span = debug_span!(parent: parent.clone(), "request_error");
                    let follow = span.clone();
                    let _enter = span.enter();
                    let e = opt.take().expect("Error should always be set, we use an option to make it easier to  move the error out of the state");
                    if e.is_retryable() {
                        debug!(error = %e, "recoverable error connecting to event source, will retry");
                        self.as_mut()
                            .project()
                            .state
                            .set(RetryRequestState::Retrying(follow));
                        continue;
                    } else {
                        error!(error = %e, "unrecoverable error connecting to event source");
                        self.as_mut().project().state.set(RetryRequestState::Closed);
                        break Ready(Some(Err((e.into()))));
                    }
                }
                RetryRequestStateProj::Retrying(parent) => {
                    let span = debug_span!(parent: parent.clone(), "retry");
                    let follow = span.clone();
                    let _enter = span.enter();

                    match this.backoff.next_backoff() {
                        Some(duration) => {
                            debug!(duration=?duration, "waiting before retrying");
                            self.as_mut()
                                .project()
                                .state
                                .set(RetryRequestState::WaitingForRetry(
                                    tokio::time::sleep(duration),
                                    follow,
                                ));
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
                RetryRequestStateProj::WaitingForRetry(mut sleep, parent_span) => {
                    let span = debug_span!(parent: parent_span.clone(), "retry::wait");
                    let follow = span.clone();
                    let _enter = span.enter();
                    match futures::ready!(sleep.poll_unpin(cx)) {
                        () => {
                            let span = debug_span!(parent: follow.clone(), "retrying");
                            let _enter = span.enter();
                            //debug!("retrying now");
                            self.as_mut()
                                .project()
                                .state
                                .set(RetryRequestState::New(Some(follow)));
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
            EventSourceError::Io(_) => true,
        }
    }
}
