use crate::messages::{DeleteEvent, Message, PatchEvent, PutEvent};
use anyhow::Error;
use eventsource_client::Event;

impl TryFrom<Event> for Message {
    type Error = Error;

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        match event.event_type.as_str() {
            "put" => Ok(Message::Put(serde_json::from_str(&event.data)?)),
            "patch" => Ok(Message::Patch(serde_json::from_str(&event.data)?)),
            "delete" => Ok(Message::Delete(serde_json::from_str(&event.data)?)),
            "reconnect" => Ok(Message::Reconnect),
            _ => Err(Error::msg("Unknown event type")),
        }
    }
}
