#[deny(warnings)]
mod bufext;
mod decoder;
mod diagnostic_errors;
mod encoder;
mod linescodec;

pub use decoder::{SSEDecodeError as DecodeError, SSEDecoder as Decoder};
pub use encoder::{SSEEncodeError as EncodeError, SSEEncoder as Encoder};

#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Comment(String),
    Event(Event),
    Retry(std::time::Duration),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub id: Option<String>,
    pub name: String,
    pub data: String,
}
impl Event {}
