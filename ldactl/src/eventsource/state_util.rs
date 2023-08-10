use std::pin::Pin;

use super::EventSourceError;
use futures::{Future, Stream};
use pin_project::pin_project;
use reqwest::Response;
use tokio_sse_codec::{Event, Frame};

pub(crate) type NextState = Option<EventSourceState>;

#[pin_project(project = StateProj)]
pub(crate) enum EventSourceState {
    Initial,
    ForceReconnect(tracing::span::EnteredSpan),
    New(tracing::span::EnteredSpan),
    Connect(
        Pin<Box<dyn Future<Output = Result<Response, reqwest::Error>> + Send>>,
        tracing::span::EnteredSpan,
    ),
    Connected(
        Pin<Box<dyn Stream<Item = Result<Frame, EventSourceError>> + Send>>,
        tracing::span::EnteredSpan,
    ),
    WaitingForRetry(#[pin] tokio::time::Sleep, tracing::span::EnteredSpan),
    Closed,
}

impl std::fmt::Display for EventSourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventSourceState::Initial => write!(f, "Initial"),
            EventSourceState::ForceReconnect(..) => write!(f, "ForcedReconnect"),
            EventSourceState::New(_) => write!(f, "New"),
            EventSourceState::Connect(_, _) => write!(f, "Connect"),
            EventSourceState::Connected(_, _) => write!(f, "Connected"),
            EventSourceState::WaitingForRetry(..) => write!(f, "WaitingForRetry"),
            EventSourceState::Closed => write!(f, "Closed"),
        }
    }
}
impl std::fmt::Debug for EventSourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

pub(crate) enum StateAction {
    Break(std::task::Poll<Option<Result<Event, EventSourceError>>>),
    Continue,
}

pub(crate) mod macros {

    macro_rules! run_state {
    ($target:ident, $action:ident($($args:expr),*)) => {{

        let (action, next_state) = { $target.as_mut().$action($($args,)*) };

        if let Some(next_state) = next_state {
            $target.as_mut().project().state.set(next_state.into());
        };
        match action {
            StateAction::Break(v) => break v,
            StateAction::Continue => continue,
        }

    }};

    }
    pub(crate) use run_state;
}
