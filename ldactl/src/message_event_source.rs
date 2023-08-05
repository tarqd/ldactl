use crate::messages::{DeleteEvent, Message, PatchEvent, PutEvent};
use miette::Diagnostic;
use thiserror::Error;
use tokio_sse_codec::Event;
use tracing::{error_span, instrument, Instrument};

#[derive(Debug, Error, Diagnostic)]
pub enum MessageParseError {
    #[error("unknown event type in sse stream: {}", .0.name)]
    UnknownEventType(Event),
    #[error("error parsing {0} event: {1}")]
    JSONError(&'static str, #[source] serde_json::Error),
}

const PUT_EVENT: &'static str = "put";
const PATCH_EVENT: &'static str = "patch";
const DELETE_EVENT: &'static str = "delete";
const RECONNECT_EVENT: &'static str = "reconnect";

impl TryFrom<Event> for Message {
    type Error = MessageParseError;
    #[instrument(level = "debug", fields(event_name=%event.name))]
    fn try_from(event: Event) -> Result<Self, Self::Error> {
        match event.name.as_str() {
            "put" => Ok(Message::Put(
                serde_json::from_str(&event.data)
                    .map_err(|e| MessageParseError::JSONError(PUT_EVENT, e))?,
            )),
            "patch" => Ok(Message::Patch(
                serde_json::from_str(&event.data)
                    .map_err(|e| MessageParseError::JSONError(PATCH_EVENT, e))?,
            )),
            "delete" => Ok(Message::Delete(
                serde_json::from_str(&event.data)
                    .map_err(|e| MessageParseError::JSONError(DELETE_EVENT, e))?,
            )),
            "reconnect" => Ok(Message::Reconnect),
            _ => Err(MessageParseError::UnknownEventType(event)),
        }
    }
}
