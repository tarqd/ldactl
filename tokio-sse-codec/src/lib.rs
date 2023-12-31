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
//! ```
//! use futures::StreamExt;
//! use tokio_util::codec::{FramedRead, Decoder};
//! use tokio_sse_codec::{SseDecoder, Frame, Event, SseDecodeError};
//!
//! # async fn run() -> Result<(), SseDecodeError> {
//! // you can use any stream or type that implements `AsyncRead`  
//! let data = "id: 1\nevent: example\ndata: hello, world\n\n";
//! let mut reader = FramedRead::new(data.as_bytes(), SseDecoder::<String>::new());
//!
//! while let Some(Ok(frame)) = reader.next().await {
//!     match frame {
//!         Frame::Event(event) => println!("event: id={:?}, name={}, data={}", event.id, event.name, event.data),
//!         Frame::Comment(comment) => println!("comment: {}", comment),
//!         Frame::Retry(duration) => println!("retry: {:#?}", duration),
//!     }
//! }
//! # Ok::<(), SseDecodeError>(())
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
//! let decoder  = SseDecoder::<String>::with_max_size(1024);
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
mod bytestr;
mod decoder;
mod decoder_impl;
mod encoder;
mod errors;
mod field_decoder;
mod traits;

pub use bytestr::BytesStr;
pub use decoder::{DecoderParts, SseDecoder};
pub use encoder::{SseEncodeError, SseEncoder};
pub use errors::{DecodeUtf8Error, ExceededSizeLimitError, SseDecodeError};
pub use traits::{TryFromBytesFrame, TryIntoFrame};
/// Represents a parsed frame from an SSE stream.
/// See [Interpreting an Event Stream](https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation)
pub enum Frame<T> {
    /// Should be ignored by the client.
    ///
    /// They are emitted for logging and to enable read-timeouts for consumers
    /// A common pattern is to send an empty comment at a regular interval to keep the connection alive
    Comment(T),
    /// Contains the name, data and optional id for the event.
    /// See [`crate::Event`]
    Event(Event<T>),
    /// Clients should use this value as the minimum delay before re-attempting a failed connection
    Retry(std::time::Duration),
}

impl<T> std::fmt::Debug for Frame<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Frame::Comment(comment) => write!(f, "Comment({:?})", comment),
            Frame::Event(event) => write!(f, "Event({:?})", event),
            Frame::Retry(duration) => write!(f, "Retry({:?})", duration),
        }
    }
}

impl<T> Clone for Frame<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        match self {
            Self::Comment(comment) => Self::Comment(comment.clone()),
            Self::Event(event) => Self::Event(event.clone()),
            Self::Retry(retry) => Self::Retry(*retry),
        }
    }
}
impl<T> PartialEq for Frame<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Comment(lhs), Self::Comment(rhs)) => lhs.eq(rhs),
            (Self::Event(lhs), Self::Event(rhs)) => lhs.eq(rhs),
            (Self::Retry(lhs), Self::Retry(rhs)) => lhs.eq(rhs),
            _ => false,
        }
    }
}
impl<T> Eq for Frame<T> where T: Eq {}
impl<T> PartialOrd for Frame<T>
where
    T: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::Comment(lhs), Self::Comment(rhs)) => lhs.partial_cmp(rhs),
            (Self::Event(lhs), Self::Event(rhs)) => lhs.partial_cmp(rhs),
            (Self::Retry(lhs), Self::Retry(rhs)) => lhs.partial_cmp(rhs),
            _ => None,
        }
    }
}

impl<T> std::hash::Hash for Frame<T>
where
    T: std::hash::Hash,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::Comment(comment) => comment.hash(state),
            Self::Event(event) => event.hash(state),
            Self::Retry(retry) => retry.hash(state),
        }
    }
}

/// Represents an SSE event.
pub struct Event<T> {
    /// Clients should record this value and send it on future connections as the `Last-Event-ID` header.
    /// If no id has been set, this property is `None`.
    ///
    /// If another event is dispatched without an `id` field, the previous `id` will be used.
    ///
    /// See [Last-Event-ID](https://html.spec.whatwg.org/multipage/server-sent-events.html#last-event-id)
    pub id: Option<std::borrow::Cow<'static, str>>,
    /// If no `name` field is sent by the stream, `"message"` will be used.
    pub name: std::borrow::Cow<'static, str>,
    /// Contains the value of all of the `data` fields received for this event joined by a newline (`'\n'`).
    pub data: T,
}
impl<T> Clone for Event<T>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            name: self.name.clone(),
            data: self.data.clone(),
        }
    }
}
impl<T> PartialEq for Event<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.name == other.name && self.data == other.data
    }
}
impl<T> Eq for Event<T> where T: Eq {}

impl<T> std::fmt::Debug for Event<T>
where
    T: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Event")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("data", &self.data)
            .finish()
    }
}
impl<T> PartialOrd for Event<T>
where
    T: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (&self.id, &self.name, &self.data).partial_cmp(&(&other.id, &other.name, &other.data))
    }
}

impl<T> Ord for Event<T>
where
    T: Ord,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.id, &self.name, &self.data).cmp(&(&other.id, &other.name, &other.data))
    }
}
impl<T> std::hash::Hash for Event<T>
where
    T: std::hash::Hash,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.name.hash(state);
        self.data.hash(state);
    }
}
