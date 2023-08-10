//! Server-Sent Event Streams Codec
//!
//! Implements a [`Codec`] for encoding and decoding [Server-Sent Events] streams.
//!
//! Advantages:
//! - Minimizes allocations by using the buffer provided by [`FramedWrite`] and [`FramedRead`] while parsing lines
//! - Easy to use with the rest of the tokio ecosystem
//! - Can be used with any type that implements [`AsyncRead`] or [`AsyncWrite`]
//! - Errors implement [`miette::Diagnostic`] for better error and diagnostic messages
//!
//! # Quick Links
//!
//! - [`SseDecoder`] - turns a bytes into [Frames][`Frame`]
//! - [`SseEncoder`] - turns [Frames][`Frame`] into bytes
//! - [`Frame`] - A parsed frame from an SSE stream containing either an event, comment or retry value
//! - [`Event`] - SSE Event containing the name, data and optional id

//! # Examples
//!
//! ```rust
//! use futures::StreamExt;
//! use tokio::io::AsyncRead;
//! use tokio_util::codec::{FramedRead, Decoder};
//! use tokio_sse_codec::{SseDecoder, Frame, Event, SseDecodeError};
//!
//! # async fn run() -> Result<(), SseDecodeError> {
//!     // you can use any stream or type that implements `AsyncRead`  
//!     let data = "id: 1\nevent: example\ndata: hello, world\n\n";
//!     let mut reader = FramedRead::new(data.as_bytes(), SseDecoder::new());
//!
//!     while let Some(Ok(frame)) = reader.next().await {
//!          match frame {
//!               Frame::Event(event) => println!("event: id={:?}, name={}, data={}", event.id, event.name, event.data),
//!               Frame::Comment(comment) => println!("comment: {}", comment),
//!               Frame::Retry(duration) => println!("retry: {:#?}", duration),
//!          }
//!     }
//!     # Ok::<(), SseDecodeError>(())
//!# }
//! ```
//!
//! ## Setting a buffer size limit
//!
//! By default, the decoder will not limit the size of the buffer used to store the data of an event.
//! It's recommended to set one when dealing with untrusted input, otherwise a malicious server could send a very large event and consume all available memory.
//!
//! The buffer should be able to hold a single event and it's data.
//!
//! ```rust
//! use tokio_sse_codec::SseDecoder;
//!
//! let decoder = SseDecoder::with_max_size(1024);
//! ```
//!
//! [Server-Sent Events]: https://html.spec.whatwg.org/multipage/server-sent-events.html#server-sent-events
//! [`AsyncRead`]: ../tokio/io/trait.AsyncRead.html
//! [`AsyncWrite`]: ../tokio/io/trait.AsyncRead.html
//! [`Codec`]: tokio_util::codec
//! [`FramedRead`]: tokio_util::codec::FramedRead
//! [`FramedWrite`]: tokio_util::codec::FramedWrite
//! [`framed`]: tokio_util::codec::Decoder::framed
//! [`Encoder`]: tokio_util::codec::Encoder
//! [`Decoder`]: tokio_util::codec::Decoder
//!
#![deny(warnings)]
#![deny(missing_docs)]
mod bufext;
mod decoder;
mod encoder;
mod errors;

pub use decoder::{DecoderParts, SseDecoder};
pub use encoder::{SseEncodeError, SseEncoder};
pub use errors::{DecodeUtf8Error, ExceededSizeLimitError, SseDecodeError};

/// Represents a parsed frame from an SSE stream.
/// See [Interpreting an Event Stream](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    /// Should be ignored by the client.
    ///
    /// They are emitted for logging and to enable read-timeouts for consumers
    /// A common pattern is to send an empty comment at a regular interval to keep the connection alive
    Comment(String),
    /// Contains the name, data and optional id for the event.
    /// See [`crate::Event`]
    Event(Event),
    /// Clients should use this value as the minimum delay before re-attempting a failed connection
    Retry(std::time::Duration),
}

/// Represents an SSE event.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Clients should record this value and send it on future connections as the `Last-Event-ID` header.
    /// If no id has been set, this property is `None`.
    ///
    /// If another event is dispatched without an `id` field, the previous `id` will be used.
    ///
    /// See [Last-Event-ID](https://html.spec.whatwg.org/multipage/server-sent-events.html#last-event-id)
    pub id: Option<String>,
    /// If no `name` field is sent by the stream, `"message"` will be used.
    pub name: String,
    /// Contains the value of all of the `data` fields received for this event joined by a newline (`'\n'`).
    pub data: String,
}
