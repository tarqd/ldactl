#![deny(warnings)]
#![deny(missing_docs)]
use crate::{errors::DecodeUtf8Error, Event, Frame};

use bytes::{BufMut, BytesMut};
use miette::Diagnostic;
use thiserror::Error;
use tokio_util::codec::Encoder;
use tracing::instrument;

/// Encodes SSE [`Frame`]s into bytes
///
/// # Examples
/// ```
/// use tokio_sse_codec::{SseEncoder, Frame, Event};
/// use tokio_util::codec::Encoder;
/// use bytes::BytesMut;
///
/// let mut encoder = SseEncoder::new();
/// let mut buf = BytesMut::new();
/// encoder.encode(&Frame::Event(Event {
///    id: Some("1".to_string()),
///    name: "example".to_string(),
///    data: "hello, world".to_string(),
/// }), &mut buf).unwrap();
///
/// let result = String::from_utf8(buf.to_vec()).unwrap();
///
/// assert_eq!(result, "id: 1\nevent: example\ndata: hello, world\n\n");
/// ```
/// [`tokio::io::AsyncWrite`]: ../tokio/io/trait.AsyncWrite.html
#[derive(Debug, PartialEq, Eq)]
pub struct SseEncoder {}
impl SseEncoder {
    /// Creates a new [`SseEncoder`]
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SseEncoder {
    // Creates a new [`SseEncoder`] with default settings
    // Today there are no settings so this is the same as [`SseEncoder::new`] but it's here for future compatibility
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Error, Diagnostic, Debug)]
/// Error returned by [`SseEncoder::encode`]
pub enum SseEncodeError {
    /// An i/o error occurred while writing the destination
    #[error("i/o error while writing stream")]
    Io(#[from] std::io::Error),
    /// The data of an event contained invalid utf-8
    #[error("invalid utf-8")]
    Utf8(#[from] DecodeUtf8Error),
}

impl Encoder<&Frame> for SseEncoder {
    type Error = SseEncodeError;
    #[instrument(skip(dst), err)]
    fn encode(&mut self, item: &Frame, dst: &mut BytesMut) -> Result<(), SseEncodeError> {
        match item {
            Frame::Comment(comment) => {
                // we may overallocate a little bit here for multi-line comments
                // but that's fine, the buffer gets re-used
                let line_count = comment.lines().count();
                let count = ((b": \n".len()) * std::cmp::min(line_count, 1)) + comment.len();
                dst.reserve(count);
                for line in comment.lines() {
                    dst.extend_from_slice(b": ");
                    dst.extend_from_slice(line.as_bytes());
                    dst.extend_from_slice(b"\n");
                }
            }
            Frame::Event(Event {
                ref id,
                ref name,
                ref data,
            }) => {
                let count = {
                    let mut count = 0usize;
                    count += id.as_ref().map_or(0, |id| b"id: \n".len() + id.len());
                    count += name.len() + b"event: \n".len();
                    let line_count = data.lines().count();
                    count += (b"data: \n".len()) * std::cmp::min(line_count, 1);
                    count += data.len();
                    count += 2; // \n\n
                    count
                };

                dst.reserve(count);

                if let Some(id) = id {
                    dst.extend_from_slice(b"id: ");
                    dst.extend_from_slice(id.as_bytes());
                    dst.extend_from_slice(b"\n");
                }

                dst.extend_from_slice(b"event: ");
                dst.extend_from_slice(name.as_bytes());
                dst.extend_from_slice(b"\n");

                for data in data.lines() {
                    dst.extend_from_slice(b"data: ");
                    dst.put(data.as_bytes());
                    dst.extend_from_slice(b"\n");
                }

                dst.extend_from_slice(b"\n");
            }
            Frame::Retry(retry) => {
                let retry = retry.as_millis();
                let count =
                    b"retry: \n".len() + ((retry.checked_ilog10().unwrap_or(0) + 1) as usize);
                dst.reserve(count);
                dst.extend_from_slice(b"retry: ");
                dst.extend_from_slice(retry.to_string().as_bytes());
                dst.extend_from_slice(b"\n");
            }
        }
        Ok(())
    }
}
