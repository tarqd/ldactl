#[deny(warnings)]
use super::{
    bufext::{BufExt, Utf8DecodeDiagnostic},
    diagnostic_errors::UTF8Error,
    linescodec::{LinesCodec, LinesCodecError},
    Event, Frame,
};

use bytes::{Buf, BufMut, BytesMut};
use miette::Diagnostic;
use thiserror::Error;
use tokio_util::codec::Decoder;
use tracing::{instrument, trace, warn};

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

#[derive(Clone, Debug, PartialEq)]
pub struct SSEDecoder {
    line_codec: LinesCodec,
    data_buf: BytesMut,
    event_type: String,
    event_id: String,
    line_count: usize,
}

#[derive(Error, Diagnostic, Debug)]
pub enum SSEDecodeError {
    #[error("i/o error while reading stream")]
    Io(#[from] std::io::Error),
    #[error("unexpected end of stream")]
    #[diagnostic(help("the stream was closed by the source before completing the last event. this may be due to a disconnection or issue with the source."))]
    UnexpectedEof,
    #[error("{1}")]
    Utf8Error(#[source] UTF8Error, String),
    #[error("error from line codec")]
    LineCodecError(#[source] LinesCodecError),
}

impl SSEDecoder {
    /// Returns an `SSECodec`
    ///
    /// # Note
    ///
    /// Setting a length limit is highly recommended for any `SSECodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the line currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters, causing unbounded memory consumption.
    ///
    /// [`LinesCodecError`]: tokio-util::codec::LinesCodecError
    pub fn new(last_id: Option<String>, max_line_len: Option<usize>) -> Self {
        Self {
            line_codec: max_line_len
                .map(|v| LinesCodec::new_with_max_length(v))
                .unwrap_or_else(|| LinesCodec::new()),
            data_buf: BytesMut::new(),
            event_type: String::new(),
            event_id: last_id.unwrap_or_default(),
            line_count: 0usize,
        }
    }

    /// Returns a `SSECodec` with a maximum line length limit.
    ///
    /// If this is set, calls to `SSECodec::decode` will return a
    /// [`LinesCodecError`] when a line exceeds the length limit. Subsequent calls
    /// will discard up to `limit` bytes from that line until a newline
    /// character is reached, returning `None` until the line over the limit
    /// has been fully discarded. After that point, calls to `decode` will
    /// function as normal.
    ///
    /// # Note
    ///
    /// Setting a length limit is highly recommended for any `LinesCodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the line currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters, causing unbounded memory consumption.
    ///
    /// [`LinesCodecError`]: tokio-util::codec::LinesCodecError
    pub fn with_max_line_length(max_line_length: usize) -> Self {
        Self::new(None, Some(max_line_length))
    }

    /// Returns the current value of the event id buffer
    /// This value is set by when `id` field is received
    /// It is not cleared after an event is dispatched
    pub fn last_event_id(&self) -> Option<&'_ str> {
        if self.event_id.is_empty() {
            None
        } else {
            Some(&self.event_id)
        }
    }
    // Returns the current value of the event type buffer
    // This value is set by when `event` field is received
    // It is cleared when an event is dispatched
    // Defaults to `message` if not set
    pub fn current_event_type(&self) -> &'_ str {
        static MESSAGE: &str = "message";
        if self.event_type.is_empty() {
            MESSAGE
        } else {
            &self.event_type
        }
    }
    /// Returns the maximum line length when decoding.
    pub fn max_length(&self) -> usize {
        self.line_codec.max_length()
    }
}

impl Decoder for SSEDecoder {
    type Item = Frame;

    type Error = SSEDecodeError;
    #[instrument(skip(self, src), err)]
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // check for utf8 bom at the start of the first line
        trace!(buflen = src.len(), "attempting decode with line codec");
        if self.line_count == 0 {
            src.strip_utf8_bom();
        }
        // Returning the loop expression let's use use break <result> to return
        return loop {
            //
            // Attempt to all available lines from the linecodec until we
            // 1. Need more data
            // 2. Can emit an event
            // 3. Need to bail with an error and close the stream

            let mut src = match self.line_codec.decode(src) {
                Ok(Some(line)) => line,
                Ok(None) => break Ok(None),
                Err(e) => break Err(e.into()),
            };
            self.line_count += 1;
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
                if self.data_buf.ends_with(b"\n") {
                    self.data_buf.truncate(self.data_buf.len() - 1);
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
                // We check utf-8 validity here since we're combing data from across lines
                let data_buf = self
                    .data_buf
                    .decode_utf8_owned()
                    .into_decode_diagnostic("failed to decode data buffer during event dispatch")?;
                self.data_buf.clear();

                // Ready to emit :)
                break Ok(Some(Frame::Event(Event {
                    id: event_id,
                    name: event_type,
                    data: data_buf,
                })));
            }
            //
            // Comment dispatch
            //
            if src[0] == b':' {
                src.advance(1);
                src.advance_if(b' ');

                break Ok(Some(Frame::Comment(
                    src.decode_utf8_owned()
                        .into_decode_diagnostic("failed to decode comment")?,
                )));
            }

            //
            // Decode field
            //

            // Spec says that if there's no colon, treat the whole line like the field name
            let colon_pos = src.find_pos(b':').unwrap_or(src.len());
            let field = src.split_to(colon_pos);
            let mut value = src;
            // Advance past the semicolon and a single space (if it's there)
            value.advance(1);
            value.advance_if(b' ');

            let value = value
                .decode_utf8()
                .into_decode_diagnostic("failed to decode field value")?;

            //
            // Field dispatch
            //
            // The steps to process the field given a field name and a field value depend on the field name
            // Field names must be compared literally, with no case folding performed.
            //
            match field.as_ref() {
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
                    self.data_buf.put(value.as_bytes());
                    self.data_buf.put_u8(b'\n');
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
                        self.event_id.push_str(value);
                    } else {
                        warn!(
                            field = "id",
                            value,
                            "ignore invalid value (reason: `id` must not contain null bytes)"
                        );
                    }
                    continue;
                }
                // If the field name is "retry"
                // -> If the field value consists of only ASCII digits,
                //    then interpret the field value as an integer in base ten,
                //    and set the event stream's reconnection time to that integer.
                // -> Otherwise, ignore the field.
                b"retry" => {
                    let retry = value
                        .parse()
                        .ok()
                        .map(std::time::Duration::from_millis)
                        .map(Frame::Retry);

                    if retry.is_none() {
                        warn!(
                            field = "retry",
                            value, "ignoring invalid value (reason: failed to parse as duration)"
                        );
                    }
                    break Ok(retry);
                }
                // Otherwise
                // The field is ignored.
                _ => {
                    let field = String::from_utf8_lossy(field.as_ref());
                    warn!(field = field.as_ref(), "ignoring unknown field");
                    continue;
                }
            }
        };
    }

    #[instrument(skip(self, buf), err)]
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

impl Default for SSEDecoder {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl From<LinesCodecError> for SSEDecodeError {
    fn from(err: LinesCodecError) -> Self {
        match err {
            LinesCodecError::Io(e) => Self::Io(e),
            _ => Self::LineCodecError(err),
        }
    }
}

// Error utility traits
trait IntoDecodeError {
    fn into_decode_error(self, msg: &str) -> SSEDecodeError;
}

trait IntoDecodeDiagnostic<T, E>
where
    E: std::error::Error + Sized,
{
    fn into_decode_diagnostic(self, msg: &str) -> Result<T, E>;
}
impl<T> IntoDecodeDiagnostic<T, SSEDecodeError> for Result<T, UTF8Error> {
    fn into_decode_diagnostic(self, msg: &str) -> Result<T, SSEDecodeError> {
        self.map_err(|e| e.into_decode_error(msg))
    }
}

impl IntoDecodeError for UTF8Error {
    fn into_decode_error(self, msg: &str) -> SSEDecodeError {
        SSEDecodeError::Utf8Error(self, msg.to_string())
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use futures::StreamExt;
    use tokio_util::codec::FramedRead;

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
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await.unwrap().unwrap();
        let decoder = framed.decoder();
        // should reset after event dispatch
        assert_eq!(decoder.current_event_type(), "message");
        assert_eq!(
            event,
            Frame::Event(Event {
                id: None,
                name: "foo".to_owned(),
                data: "bar".to_owned()
            })
        );
    }
    #[tokio::test]
    async fn test_current_event_type() {
        let bytes = b"event: foo\ndata: bar\nevent: baz\n";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let _ = framed.next().await;
        let decoder = framed.decoder();

        assert_eq!(decoder.current_event_type(), "baz");
    }
    #[tokio::test]
    async fn test_event_retry() {
        let bytes = b"retry: 100\n";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await.unwrap().unwrap();

        assert_eq!(event, Frame::Retry(std::time::Duration::from_millis(100)));
    }
    #[tokio::test]
    async fn test_event_retry_invalid() {
        let bytes = b"retry: foo\n";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await;

        assert_eq!(event.is_none(), true);
    }
    #[tokio::test]
    async fn event_has_id() {
        let bytes = b"id: 1\nevent: foo\ndata: bar\n\n";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await.unwrap().unwrap();
        let decoder = framed.decoder();
        assert!(decoder.last_event_id().is_some());
        assert_eq!(decoder.last_event_id().unwrap(), "1");
        if let Frame::Event(Event { id, .. }) = event {
            assert_eq!(id, Some("1".to_owned()));
        } else {
            panic!("Expected event");
        }
    }
    #[tokio::test]
    async fn require_new_line() {
        let bytes = b"event: foo\ndata: bar";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await.unwrap();

        match event {
            Ok(_) => assert!(false, "Expected error"),
            Err(SSEDecodeError::UnexpectedEof) => (),
            Err(_) => assert!(false, "unexpected error"),
        }
    }
}
