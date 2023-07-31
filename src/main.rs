mod credential;
mod message_event_source;
mod messages;
use eventsource_client as es;
use futures::{ready, stream::FusedStream, Stream, StreamExt, TryStreamExt};
use hyper::StatusCode;
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use messages::Message;
use miette::{miette, Error, Result};

use eventsource_client::Error as EventSourceError;
use pin_project::pin_project;
use pretty_env_logger;
use std::{
    env,
    ops::{Deref, DerefMut},
    pin::Pin,
    process,
    time::Duration,
};
use thiserror::Error;
#[pin_project]
struct AutoConfigClient {
    client_factory: Box<dyn Fn() -> Box<dyn es::Client>>,
    client: Box<dyn es::Client>,
    #[pin]
    state: AutoConfigClientState,
}
#[derive(Debug, thiserror::Error)]
enum ESErrorWrapper {
    #[error("request timed out")]
    TimedOut,
    #[error("stream was closed due to an unrecoverable error.")]
    StreamClosed,
    #[error("invalid request parameter. {0}")]
    InvalidParameter(UndynError),
    /// The HTTP response could not be handled.
    #[error("unexpected response status code: {0}")]
    UnexpectedResponse(hyper::StatusCode),
    /// An error reading from the HTTP response body.
    #[error("error reading from the HTTP response body. {0}")]
    HttpStream(UndynError),
    /// The HTTP response stream ended
    #[error("received eof from http response stream")]
    Eof,
    /// The HTTP response stream ended unexpectedly (e.g. in the
    /// middle of an event).
    #[error("received unexpected eof from http response stream")]
    UnexpectedEof,
    /// Encountered a line not conforming to the SSE protocol.
    #[error("invalid line while parsing sse stream: {0}")]
    InvalidLine(String),
    #[error("invalid event while parsing sse stream")]
    InvalidEvent,
    /// Encountered a malformed Location header.
    #[error("malformed location header during redirect: {0}")]
    MalformedLocationHeader(UndynError),
    /// Reached maximum redirect limit after encountering Location headers.
    #[error("maximum redirect limit exceeded after {0} redirects ")]
    MaxRedirectLimitReached(u32),
    /// An unexpected failure occurred.
    #[error("unexpected error: {0}")]
    Unexpected(UndynError),
}

#[derive(Debug)]
struct UndynError {
    message: String,
    source: Option<anyhow::Error>,
}
impl std::error::Error for UndynError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source()
    }
}
impl std::fmt::Display for UndynError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
fn wrap_dyn_error(e: &Box<(dyn std::error::Error + std::marker::Send + 'static)>) -> UndynError {
    let msg = format!("{}", e.as_ref());
    let source = e.as_ref().source().map(|e| anyhow::anyhow!("{}", e));
    UndynError {
        message: msg,
        source,
    }
}

impl ESErrorWrapper {
    fn from(e: &es::Error) -> Self {
        use std::any::{Any, TypeId};
        use ESErrorWrapper::*;
        match e {
            EventSourceError::TimedOut => TimedOut,
            EventSourceError::StreamClosed => StreamClosed,
            EventSourceError::InvalidParameter(e) => InvalidParameter(wrap_dyn_error(e)),
            EventSourceError::UnexpectedResponse(e) => UnexpectedResponse(*e),
            EventSourceError::HttpStream(e) => HttpStream(wrap_dyn_error(e)),
            EventSourceError::Eof => Eof,
            EventSourceError::UnexpectedEof => UnexpectedEof,
            EventSourceError::InvalidLine(e) => InvalidLine(e.clone()),
            EventSourceError::InvalidEvent => InvalidEvent,
            EventSourceError::MalformedLocationHeader(e) => {
                MalformedLocationHeader(wrap_dyn_error(e))
            }
            EventSourceError::MaxRedirectLimitReached(limit) => MaxRedirectLimitReached(*limit),
            EventSourceError::Unexpected(e) => Unexpected(wrap_dyn_error(e)),
        }
    }
}
#[pin_project(project = StateProj)]
enum AutoConfigClientState {
    New,
    Stream(
        Pin<
            Box<
                (dyn Stream<Item = Result<es::SSE, es::Error>>
                     + std::marker::Send
                     + Sync
                     + 'static),
            >,
        >,
    ),
    StreamClosed,
}
impl AutoConfigClient {
    fn new(client_factory: Box<dyn Fn() -> Box<dyn es::Client>>) -> Self {
        let client = (client_factory)();

        Self {
            client_factory,
            client,
            state: AutoConfigClientState::New,
        }
    }
}
impl Stream for AutoConfigClient {
    type Item = Result<Message, anyhow::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        loop {
            let this = self.as_mut().project();
            let state = this.state.project();
            match state {
                StateProj::New => {
                    let client = (this.client_factory)();
                    let stream = client.stream();
                    *this.client = client;
                    self.as_mut()
                        .project()
                        .state
                        .set(AutoConfigClientState::Stream(stream));
                    continue;
                }
                StateProj::Stream(stream) => match stream.poll_next_unpin(cx) {
                    Poll::Ready(Some(Ok(sse))) => match sse {
                        es::SSE::Event(event) => match event.event_type.as_str() {
                            "reconnect" => {
                                self.as_mut()
                                    .project()
                                    .state
                                    .set(AutoConfigClientState::New);
                                continue;
                            }
                            _ => match Message::try_from(event) {
                                Ok(msg) => return Poll::Ready(Some(Ok(msg))),
                                Err(e) => {
                                    return Poll::Ready(Some(Err(e)));
                                }
                            },
                        },
                        es::SSE::Comment(_) => continue,
                    },
                    Poll::Ready(Some(Err(e))) => {
                        return Poll::Ready(Some(Err(anyhow::Error::new(ESErrorWrapper::from(&e))
                            .context("unrecoverable error from sse client"))))
                    }
                    Poll::Ready(None) => {
                        self.as_mut()
                            .project()
                            .state
                            .set(AutoConfigClientState::StreamClosed);
                        continue;
                    }
                    Poll::Pending => return Poll::Pending,
                },
                StateProj::StreamClosed => return Poll::Ready(None),
            }
        }
    }
}

impl FusedStream for AutoConfigClient {
    fn is_terminated(&self) -> bool {
        matches!(self.state, AutoConfigClientState::StreamClosed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_config_init() {}
    #[test]
    fn it_works() {
        assert!(true)
    }
}

#[tokio::main]
async fn main() -> Result<(), es::Error> {
    pretty_env_logger::init();
    let mut do_connect = true;
    while do_connect {
        do_connect = false;
        let auth_header = "rel-b81f0150-f475-4638-8b79-fa1554c635bb";
        let url = "https://stream.launchdarkly.com/relay_auto_config";
        let client = es::ClientBuilder::for_url(url)?
            .header("Authorization", auth_header)?
            .reconnect(
                es::ReconnectOptions::reconnect(true)
                    .retry_initial(false)
                    .delay(Duration::from_secs(1))
                    .backoff_factor(2)
                    .delay_max(Duration::from_secs(60))
                    .build(),
            )
            .build();

        let mut stream = tail_events(client);

        while let Ok(Some(Ok(message))) = stream.try_next().await {
            println!("hi there");
            match message {
                Message::Put(put) => {
                    debug!("Put: {:#?}", put);
                }
                Message::Patch(patch) => {
                    debug!("Patch: {:#?}", patch);
                }
                Message::Delete(delete) => {
                    debug!("Delete: {:#?}", delete);
                }
                Message::Reconnect => {
                    debug!("Reconnect");
                    do_connect = true;

                    break;
                }
                Message::Comment(comment) => {
                    info!("Comment: {:?}", comment);
                }
            }
        }
    }
    Ok(())
}

fn tail_events(
    client: impl es::Client,
) -> impl Stream<Item = Result<Result<Message, anyhow::Error>, anyhow::Error>> {
    client
        .stream()
        .map_ok(|event| {
            debug!("Event: {:?}", event);
            match event {
                es::SSE::Event(ev) => Message::try_from(ev),
                es::SSE::Comment(comment) => Ok(Message::Comment(comment)),
            }
        })
        .map_err(|e| anyhow::anyhow!("Error: {:?}", e))
}
