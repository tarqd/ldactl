use std::{borrow::BorrowMut, io::Write, ops::Range};

use bytes::{Buf, Bytes, BytesMut};
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LinesCodec, LinesCodecError};
/*
stream        = [ bom ] *event
event         = *( comment / field ) end-of-line
comment       = colon *any-char end-of-line
field         = 1*name-char [ colon [ space ] *any-char ] end-of-line
end-of-line   = ( cr lf / cr / lf )

; characters
lf            = %x000A ; U+000A LINE FEED (LF)
cr            = %x000D ; U+000D CARRIAGE RETURN (CR)
space         = %x0020 ; U+0020 SPACE
colon         = %x003A ; U+003A COLON (:)
bom           = %xFEFF ; U+FEFF BYTE ORDER MARK
name-char     = %x0000-0009 / %x000B-000C / %x000E-0039 / %x003B-10FFFF
                ; a scalar value other than U+000A LINE FEED (LF), U+000D CARRIAGE RETURN (CR), or U+003A COLON (:)
any-char      = %x0000-0009 / %x000B-000C / %x000E-10FFFF
                ; a scalar value other than U+000A LINE FEED (LF) or U+000D CARRIAGE RETURN (CR)
*/

// We only support UTF-8 in this house
const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

#[derive(Clone, Debug, PartialEq)]
pub enum DataKind {
    Comment,
    Event(Bytes),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Comment(String),
    Event {
        id: Option<String>,
        name: String,
        data: String,
    },
    Retry(u64),
}

impl Event {}

#[derive(Clone, Debug, PartialEq)]
pub struct SSECodec {
    line_codec: LinesCodec,
    line_count: usize,
    current_line: Option<String>,
    data_buf: Vec<u8>,
    event_type: String,
    event_id: String,
}

#[derive(Error, Diagnostic, Debug)]
pub enum SSEDecodeError {
    #[error("invalid event in stream")]
    #[diagnostic(code(sse::invalid_event))]
    #[diagnostic(help("make sure the source is sending properly formatted SSE events."))]
    InvalidEvent,
    #[error("failed to decode next line")]
    LineCodecError(#[from] LinesCodecError),
    #[error("i/o error while reading stream")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of stream")]
    #[diagnostic(code(sse::unexpected_eof))]
    #[diagnostic(help("the stream was closed by the source before completing the last event. make sure the source is sending valid SSE events"))]
    UnexpectedEof,
    #[error("unexpected error")]
    Unexpected(#[source] Box<dyn std::error::Error>),
    #[error("invalid utf-8")]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error("attempt to decode after error")]
    DecoderClosed,
}

impl SSECodec {
    pub fn new() -> Self {
        Self {
            line_codec: LinesCodec::new(),
            line_count: 0usize,
            data_buf: Vec::new(),
            current_line: None,
            event_type: String::new(),
            event_id: String::new(),
        }
    }
    pub fn consume_data(&mut self) -> Result<String, SSEDecodeError> {
        // We create a string by reference so we can keep the buffers
        // capacity for parsing future events.
        let ret = std::str::from_utf8(&self.data_buf).map(str::to_owned);
        self.data_buf.clear();
        // We don't want to use ? before we clear the buffer
        // Otherwise, the invalid data will get merged with future events
        Ok(ret?)
    }
}

trait BufExt {
    fn find(&self, byte: u8) -> Option<usize>;
    fn advance_if(&mut self, byte: u8);
}
impl BufExt for &[u8] {
    #[inline]
    fn advance_if(&mut self, byte: u8) {
        if !self.is_empty() && self[0] == byte {
            self.advance(1)
        }
    }
    #[inline]
    fn find(&self, byte: u8) -> Option<usize> {
        self.iter().position(|b| *b == byte)
    }
}

impl Decoder for SSECodec {
    type Item = Event;

    type Error = SSEDecodeError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // Returning the loop expression let's use use break <result> to return
        return loop {
            //
            // Attempt to all available lines from the linecodec until we
            // 1. Need more data
            // 2. Can emit an event
            // 3. Need to bail with an error and close the stream

            let current_line = match self.line_codec.decode(src) {
                Ok(Some(line)) => line,
                Ok(None) => break Ok(None),
                Err(e) => break Err(e.into()),
            };

            // TODO: Use this for better diagnostics
            self.line_count += 1;
            let mut src = current_line.as_bytes();

            //
            // Strip UTF8 BOM
            //

            if src.starts_with(UTF8_BOM) {
                src.advance(UTF8_BOM.len() + 1);
            }

            //
            // Event Dispatch
            //
            if src.is_empty() {
                //If the data buffer is an empty string,
                // set the data buffer and the event type buffer
                // to the empty string and return.
                if self.data_buf.is_empty() {
                    self.event_type.clear();
                    break Ok(None);
                }
                // If the data buffer's last character is a U+000A LINE FEED (LF) character,
                // then remove the last character from the data buffer.
                if self.data_buf[self.data_buf.len() - 1] == b'\n' {
                    self.data_buf.pop();
                }
                // Use the last id we saw
                let event_id = (!self.event_id.is_empty()).then_some(self.event_id.clone());
                let event_type = {
                    if self.event_type.is_empty() {
                        String::from("message")
                    } else {
                        self.event_type.clone()
                    }
                };
                // Reset event type but not the id
                self.event_type.clear();
                // Consumes and resets the data buffer
                let data_buf = self.consume_data()?;
                // Ready to emit :)
                break Ok(Some(Event::Event {
                    id: event_id,
                    name: event_type,
                    data: data_buf,
                }));
            }
            //
            // Comment dispatch
            //
            if src[0] == b':' {
                src.advance(1);
                src.advance_if(b' ');

                break Ok(Some(Event::Comment(
                    unsafe { std::str::from_utf8_unchecked(src) }.to_owned(),
                )));
            }

            //
            // Decode field
            //

            // Spec says that if there's no colon, treat the whole line like the field name
            let colon_pos = src.find(b':').unwrap_or(src.len());
            let (field, mut value) = src.split_at(colon_pos);
            // Advance past the semicolon and a single space (if it's there)
            value.advance(1);
            value.advance_if(b' ');

            // Safe because the line we got from the codec is guaranteed utf-8
            let value = unsafe { std::str::from_utf8_unchecked(value) };

            //
            // Field dispatch
            //
            // The steps to process the field given a field name and a field value depend on the field name
            // Field names must be compared literally, with no case folding performed.
            //
            match field {
                // If the field name is "event"
                //   -> Set the event type buffer to field value.
                b"event" => {
                    self.event_type.clear();
                    self.event_type.push_str(value);
                    continue;
                }
                // If the field name is "data"
                // -> Append the field value to the data buffer,
                //    then append a single U+000A LINE FEED (LF) character to the data buffer.
                b"data" => {
                    self.data_buf.extend_from_slice(value.as_bytes());
                    self.data_buf.push(b'\n');
                    continue;
                }
                // If the field name is "id"
                // -> If the field value does not contain U+0000 NULL,
                //    then set the last event ID buffer to the field value.
                // -> Otherwise, ignore the field.
                b"id" => {
                    if !value.as_bytes().contains(&b'\0') {
                        // TODO: not sure what to do if there's an empty id
                        self.event_id.clear();
                        self.event_id.push_str(value)
                    }
                    continue;
                }
                // If the field name is "retry"
                // -> If the field value consists of only ASCII digits,
                //    then interpret the field value as an integer in base ten,
                //    and set the event stream's reconnection time to that integer.
                // -> Otherwise, ignore the field.
                b"retry" => break Ok(value.parse().ok().map(Event::Retry)),
                // Otherwise
                // The field is ignored.
                _ => continue,
            }
        };
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(SSEDecodeError::UnexpectedEof.into())
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use futures::{future, StreamExt};

    use super::*;

    #[tokio::test]
    async fn empty_lines() {
        let bytes = b"event: foo\ndata: bar\n\nhi";
        let mut framed = FramedRead::new(&bytes[..], LinesCodec::new());
        let (first, second, third, fourth) = (
            framed.next().await.unwrap().unwrap(),
            framed.next().await.unwrap().unwrap(),
            framed.next().await.unwrap().unwrap(),
            framed.next().await.unwrap().unwrap(),
        );
        assert_eq!(first, "event: foo");
        assert_eq!(second, "data: bar");
        assert_eq!(third, "");
        assert_eq!(fourth, "hi");
    }
    #[tokio::test]
    async fn test_event() {
        let bytes = b"event: foo\ndata: bar\n\n";
        let mut framed = FramedRead::new(&bytes[..], SSECodec::new());
        let event = framed.next().await.unwrap().unwrap();

        assert_eq!(
            event,
            Event::Event {
                id: None,
                name: "foo".to_owned(),
                data: "bar".to_owned()
            }
        );
    }
    #[tokio::test]
    async fn test_event_retry() {
        let bytes = b"retry: 100\n";
        let mut framed = FramedRead::new(&bytes[..], SSECodec::new());
        let event = framed.next().await.unwrap().unwrap();

        assert_eq!(event, Event::Retry(100));
    }
    #[tokio::test]
    async fn test_event_retry_invalid() {
        let bytes = b"retry: foo\n";
        let mut framed = FramedRead::new(&bytes[..], SSECodec::new());
        let event = framed.next().await;

        assert_eq!(event.is_none(), true);
    }
    #[tokio::test]
    async fn event_has_id() {
        let bytes = b"id: 1\nevent: foo\ndata: bar\n\n";
        let mut framed = FramedRead::new(&bytes[..], SSECodec::new());
        let event = framed.next().await.unwrap().unwrap();
        if let Event::Event { id, .. } = event {
            assert_eq!(id, Some("1".to_owned()));
        } else {
            panic!("Expected event");
        }
    }
    #[tokio::test]
    async fn require_new_line() {
        let bytes = b"event: foo\ndata: bar";
        let mut framed = FramedRead::new(&bytes[..], SSECodec::new());
        let event = framed.next().await.unwrap();

        match event {
            Ok(_) => assert!(false, "Expected error"),
            Err(SSEDecodeError::UnexpectedEof) => (),
            Err(_) => assert!(false, "unexpected error"),
        }
    }
}
