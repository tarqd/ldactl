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
/// let frame : Frame<String> = Frame::Event(Event {
///    id: Some("1".into()),
///    name: "example".into(),
///    data: "hello, world".into(),
/// });
/// encoder.encode(frame, &mut buf).unwrap();
///
/// let result = String::from_utf8(buf.to_vec()).unwrap();
///
/// assert_eq!(result, "id: 1\nevent: example\ndata: hello, world\n\n");
/// ```
/// [`tokio::io::AsyncWrite`]: ../tokio/io/trait.AsyncWrite.html
#[derive(Debug, Clone, PartialEq)]
pub struct SseEncoder {
    last_id: String,
}

impl SseEncoder {
    /// Creates a new [`SseEncoder`]
    pub fn new() -> Self {
        Self {
            last_id: String::new(),
        }
    }
}

impl Default for SseEncoder {
    // Creates a new [`SseEncoder`] with default settings
    // Today there are no settings so this is the same as [`SseEncoder::new`] but it's here for future compatibility
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Encoder<Frame<T>> for SseEncoder
where
    T: AsRef<[u8]>,
{
    type Error = SseEncodeError;
    #[instrument(level = "debug", skip(self, item, dst), err)]
    fn encode(&mut self, item: Frame<T>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            Frame::Comment(comment) => {
                // optimized for single line comments
                dst.reserve(comment.as_ref().len() + 1);
                let lines = comment.as_ref().split(|b| b == &b'\n');
                for line in lines {
                    dst.extend_from_slice(b": ");
                    dst.extend_from_slice(line);
                    dst.extend_from_slice(b"\n");
                }
            }
            Frame::Event(Event { id, name, data }) => {
                let id = match id {
                    Some(value) => {
                        if value != self.last_id {
                            self.last_id = value.into_owned();
                        }
                        &self.last_id
                    }
                    None => &self.last_id,
                };
                let count = {
                    let mut count = 0usize;
                    if !id.is_empty() {
                        count += b"id: \n".len() + id.len();
                    }
                    count += name.len() + b"event: \n".len();
                    count += (b"data: \n".len()) + data.as_ref().len();
                    count += 2; // \n\n
                    count
                };

                dst.reserve(count);

                if !id.is_empty() {
                    dst.extend_from_slice(b"id: ");
                    dst.extend_from_slice(id.as_bytes());
                    dst.extend_from_slice(b"\n");
                }

                dst.extend_from_slice(b"event: ");
                dst.extend_from_slice(name.as_bytes());
                dst.extend_from_slice(b"\n");
                let lines = data.as_ref().split(|b| b == &b'\n');
                for data in lines {
                    dst.extend_from_slice(b"data: ");
                    dst.put(data);
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

#[derive(Error, Diagnostic, Debug)]
/// Error returned by [`SseEncoder::encode`]
pub enum SseEncodeError {
    /// An i/o error occurred while writing the destination
    #[error("i/o error while writing stream")]
    Io(#[from] std::io::Error),
    /// The data of an event contained invalid utf-8. Not used today but might be used in the future
    #[error("invalid utf-8")]
    Utf8(#[from] DecodeUtf8Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_event() {
        let event = Frame::<String>::Event(Event {
            id: Some("1".into()),
            name: "example".into(),
            data: "hello, world".into(),
        });
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, "id: 1\nevent: example\ndata: hello, world\n\n");
    }
    #[test]
    fn id_is_sticky() {
        let event = Frame::<String>::Event(Event {
            id: Some("1".into()),
            name: "example".into(),
            data: "hello, world".into(),
        });
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let event = Frame::<String>::Event(Event {
            id: None,
            name: "example".into(),
            data: "hello, world".into(),
        });
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, "id: 1\nevent: example\ndata: hello, world\n\nid: 1\nevent: example\ndata: hello, world\n\n");
    }
    #[test]
    fn comments() {
        let event = Frame::<String>::Comment("hello, world".into());
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, ": hello, world\n");
    }
    #[test]
    fn multi_line_comment() {
        let event = Frame::<String>::Comment("hello, world\nthis is a test".into());
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, ": hello, world\n: this is a test\n");
    }
    #[test]
    fn retry() {
        let event = Frame::<String>::Retry(std::time::Duration::from_secs(1));
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, "retry: 1000\n");
    }
    #[test]
    fn retry_overflow() {
        let event = Frame::<String>::Retry(std::time::Duration::from_secs(u64::MAX));
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(result, "retry: 18446744073709551615000\n");
    }
    #[test]
    fn data_multiline() {
        let event = Frame::<String>::Event(Event {
            id: Some("1".into()),
            name: "example".into(),
            data: "hello, world\nthis is a test".into(),
        });
        let mut buf = BytesMut::new();
        let mut encoder = SseEncoder::new();
        encoder.encode(event, &mut buf).unwrap();
        let result = String::from_utf8(buf.to_vec()).unwrap();
        assert_eq!(
            result,
            "id: 1\nevent: example\ndata: hello, world\ndata: this is a test\n\n"
        );
    }
}
