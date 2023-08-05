#[deny(warnings)]
use super::{Event, Frame};

use miette::Diagnostic;
use thiserror::Error;
use tokio_util::codec::Encoder;

use bytes::{BufMut, BytesMut};
use tracing::instrument;

#[derive(Debug)]
pub struct SSEEncoder {}
impl SSEEncoder {
    #[instrument]
    pub fn new() -> Self {
        Self {}
    }
}

#[derive(Error, Diagnostic, Debug)]
pub enum SSEEncodeError {
    #[error("i/o error while writing stream")]
    Io(#[from] std::io::Error),
    #[error("invalid utf-8")]
    Utf8(#[from] std::str::Utf8Error),
}

impl Encoder<&Frame> for SSEEncoder {
    type Error = SSEEncodeError;
    #[instrument(skip(dst), err)]
    fn encode(&mut self, item: &Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
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

                dst.extend_from_slice(b"\n\n");
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
