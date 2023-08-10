#![deny(warnings)]
#![deny(missing_docs)]
use crate::bufext::BufMutExt;
use crate::errors::ExceededSizeLimit;

use super::{
    bufext::{BufExt, Utf8DecodeDiagnostic},
    errors::SSEDecodeError,
    Event, Frame,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::Decoder;
use tracing::{error, instrument, trace, warn};

/// # SSE Decoder
///
/// Decodes events from an [`AsyncRead`] into [`Frames`]
///
/// ## Quick Links
/// - [`SSEDecodeError`]: Type for unrecoverable decoder errors
/// - [`Frame`]: Type representing a parsed frame returned by [`SSEDecoder::decode`]
///
/// ## Example
///
/// ```rust
/// use bytes::BytesMut;
/// use tokio_util::codec::Decoder;
/// use tokio_sse_codec::{self as sse, Event, Frame};
///
/// let mut buffer = BytesMut::from("data: hello\n\n");
/// let mut decoder = sse::Decoder::new();
/// let frame = decoder.decode(&mut buffer);
/// assert!(matches!(frame, Ok(Some(Frame::Event(Event { id, name, data  })))));
/// ```
///
/// # SSE Format
/// As per the specification, valid SSE streams should be in the following format (BNF)
/// ```bnf
/// stream        = [ bom ] *event
/// event         = *( comment / field ) end-of-line
/// comment       = colon *any-char end-of-line
/// field         = 1*name-char [ colon [ space ] *any-char ] end-of-line
/// end-of-line   = ( cr lf / cr / lf )

/// ; characters
/// lf            = %x000A ; U+000A LINE FEED (LF)
/// cr            = %x000D ; U+000D CARRIAGE RETURN (CR)
/// space         = %x0020 ; U+0020 SPACE
/// colon         = %x003A ; U+003A COLON (:)
/// bom           = %xFEFF ; U+FEFF BYTE ORDER MARK
/// name-char     = %x0000-0009 / %x000B-000C / %x000E-0039 / %x003B-10FFFF
///                 ; a scalar value other than U+000A LINE FEED (LF), U+000D CARRIAGE RETURN (CR), or U+003A COLON (:)
/// any-char      = %x0000-0009 / %x000B-000C / %x000E-10FFFF
///                 ; a scalar value other than U+000A LINE FEED (LF) or U+000D CARRIAGE RETURN (CR)
/// ```
/// [`AsyncRead`]: ../tokio/io/trait.AsyncWrite.html
/// [`Frames`]: crate::Frame
#[derive(Clone, Debug, PartialEq)]
pub struct SSEDecoder {
    data_buf: BytesMut,
    event_type: String,
    event_id: String,
    next_line_index: usize,
    line_count: usize,
    max_buf_len: usize,
    is_closed: bool,
}

/// Tuple representing the internal buffers of the decoder
/// Most users should not use this directly unless you're re-using the buffers
/// after consuming the decoder.
///
/// Tuple contains `(data_buf, event_type, event_id, max_buf_len)`
pub type DecoderParts = (BytesMut, String, String, usize);

impl SSEDecoder {
    /// Returns an `SSECodec` with no maximum buffer size limit.
    ///
    /// # Note
    ///
    /// Setting a buffer size limit is highly recommended for any `SSECodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the line currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters, causing unbounded memory consumption.
    ///
    /// [`SSEDecodeError`]: crate::decoder::SSEDecodeError
    pub fn new() -> Self {
        Self::with_max_size(usize::MAX)
    }

    /// Returns Decoder with a maximum buffer size limits.
    ///
    /// If this is set, calls to `SSECodec::decode` will return a
    /// [`MaxBufSizeExceeded`] error if the event buffers reach this size before dispatching
    /// an event. Subsequent calls will return `None`. You should not use an encoder after it
    /// returns an error. Doing so is undefined behavior.
    ///
    /// # Note
    ///
    /// Setting a length limit is highly recommended for any `SSEEncoder` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds the event being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending an unbounded amount of input
    /// without any `\n` characters or data fields, causing unbounded memory consumption.
    ///
    /// [`MaxBufSizeExceeded`]: crate::decoder::SSEDecodeError::ExceededSizeLimit
    pub fn with_max_size(max_buf_size: usize) -> Self {
        debug_assert!(
            max_buf_size > 7,
            "max_buf_size must be greater than 7 to parse any valid SSE frame"
        );
        Self {
            data_buf: BytesMut::new(),
            event_type: String::new(),
            event_id: String::new(),
            line_count: 0usize,
            next_line_index: 0usize,
            max_buf_len: max_buf_size,
            is_closed: false,
        }
    }

    /// Returns the internal buffers and state of the decoder as a tuple
    /// This is useful for re-using the buffers when you're done with them
    /// See [`DecoderParts`]
    pub fn into_parts(self) -> DecoderParts {
        (
            self.data_buf,
            self.event_type,
            self.event_id,
            self.max_buf_len,
        )
    }
    /// Constructs a decoder from the internal buffers and state
    /// Mostly useful for testing and re-using the buffers
    /// This is unsafe because it's possible to construct an invalid decoder
    ///
    /// # Safety
    /// All data_buf, event_type, and event_id should be empty
    /// They may have capacity but should not have any data
    ///
    /// See [`DecoderParts`]
    pub unsafe fn from_parts(parts: DecoderParts) -> Self {
        let (data_buf, event_type, event_id, max_buf_size) = parts;
        Self {
            data_buf,
            event_type,
            event_id,
            line_count: 0usize,
            next_line_index: 0usize,
            max_buf_len: max_buf_size,
            is_closed: false,
        }
    }

    /// Returns the current value of the event type buffer
    /// This value is set by when `event` field is received
    /// It is cleared when an event is dispatched
    /// Defaults to `message` if not set
    pub fn current_event_type(&self) -> &'_ str {
        static MESSAGE: &str = "message";
        if self.event_type.is_empty() {
            MESSAGE
        } else {
            &self.event_type
        }
    }
    /// Returns the maximum buffer size when decoding.
    pub fn max_buf_len(&self) -> usize {
        self.max_buf_len
    }
    /// Returns the size of the internal buffers
    /// There are 3 internal buffers used while parsing an event
    /// 1. The data buffer: holds the value of any data fields received before dispatch
    /// 2. The event type buffer: holds the event name until dispatch. Reset after every dispatch
    /// 3. The event id buffer: holds the current event id. This is not reset after dispatch
    ///
    /// Internally, the decoder will keep the capacity of these buffers to avoid spurious allocations.
    /// Users will get a copy. This means we aren't constantly starting from scratch and resizing up to fit
    /// larger events.
    pub fn buf_len(&self) -> usize {
        self.data_buf.len() + self.event_id.len() + self.event_type.len()
    }

    /// Returns the remaining capacity of the internal buffers
    pub fn buf_remaining(&self) -> usize {
        self.max_buf_len.saturating_sub(self.buf_len())
    }
    /// Returns true if the decoder has been closed due to permanent error such
    /// as the buffer capacity being exceeded.
    ///
    /// When the decoder is closed, any further writes will be dropped and decode will return `None`
    pub fn is_closed(&self) -> bool {
        self.is_closed
    }

    /// Resets the decoder to a state where it can decode events after closing
    /// Calling this method is the equivalent of `let decoder = SSEDecoder::from_parts(unsafe { decoder.into_parts() })`
    ///
    /// The difference is that this method does not consume `self` and you don't need to worry about constructing an invalid decoder
    pub fn reset(&mut self) {
        self.data_buf.clear();
        self.event_type.clear();
        self.line_count = 0;
        self.next_line_index = 0;
        self.is_closed = false;
    }

    /// Clear internal buffers after closing to allow re-use via [`SSEDecoder::into_parts`]
    fn close(&mut self) {
        self.is_closed = true;
        self.data_buf.clear();
        self.event_type.clear();
    }
    /// Decodes the next line from the input
    fn decode_line(&mut self, src: &mut BytesMut) -> Result<Option<Bytes>, SSEDecodeError> {
        // if we're closed, we discard everything
        if self.is_closed {
            src.advance(src.len());
            error!("decoder is closed, discarding all input");
            return Ok(None);
        }
        let max_read_len = self.buf_remaining();
        // Determine how far into the buffer we'll search for a newline. If
        // there's no max_length set, we'll read to the end of the buffer.
        let read_to = max_read_len.min(src.len());
        let new_line_offset = src[self.next_line_index..read_to]
            .iter()
            .position(|b| *b == b'\n');
        trace!(?new_line_offset, read_to, "searching for new line");
        match new_line_offset {
            Some(offset) => {
                // we found a new line, so we can advance the buffer
                // and return the line
                let newline_index = self.next_line_index + offset;
                self.next_line_index = 0;

                let mut line = src.split_to(newline_index + 1);
                line.rbump();
                line.rbump_if(b'\r');
                self.line_count = self.line_count.saturating_add(1);
                Ok(Some(line.freeze()))
            }
            None if src.len() > max_read_len => {
                // We reached
                // so we need to discard the buffer and return an error
                src.advance(src.len());
                self.close();

                Err(SSEDecodeError::ExceededSizeLimit(ExceededSizeLimit::new(
                    self.max_buf_len,
                    src.len(),
                    self.buf_len(),
                )))
            }
            None => {
                // We didn't find a line or reach the length limit, so the next
                // call will resume searching at the current offset.
                self.next_line_index = read_to;
                Ok(None)
            }
        }
    }
}

impl Decoder for SSEDecoder {
    type Item = Frame;
    type Error = SSEDecodeError;

    /// Attempt to decode an SSE frame from the input. If we can't dispatch a frame, `Ok(None)` will be returned.
    #[instrument(skip(self, src), err)]
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, SSEDecodeError> {
        if self.line_count == 0 {
            src.strip_utf8_bom();
        }
        // Returning the loop expression let's use use break <result> to return
        return loop {
            // Attempt to all available lines from the buffer until we either
            // 1. Need more data
            // 2. Can emit an event
            // 3. Need to bail with an error and close the stream

            let mut src = match self.decode_line(src) {
                Ok(Some(line)) => line,
                Ok(None) => break Ok(None),
                Err(e) => break Err(e),
            };

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
                self.data_buf.rbump_if(b'\n');
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
                let data_buf = String::from_utf8(self.data_buf.to_vec())?;
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
                src.bump();
                src.bump_if(b' ');

                break Ok(Some(Frame::Comment(if src.is_empty() {
                    // does not allocate
                    String::new()
                } else {
                    String::from_utf8(src.to_vec())?
                })));
            }

            //
            // Decode field
            //

            // Known fields: event  (5), data (4), retry (5), id (2)
            // All valid fields will have a colon within 6 bytes
            let field = &src[0..src.len().min(6)];
            let colon_pos = {
                match field.find_byte(b':') {
                    Some(pos) => pos,
                    None => {
                        let field = String::from_utf8_lossy(src.as_ref());
                        warn!(field = field.as_ref(), "ignoring unknown field");
                        continue;
                    }
                }
            };

            let field = src.split_to(colon_pos);
            // Skip the colon and a single whitespace
            src.bump();
            src.bump_if(b' ');

            match field.as_ref() {
                // If the field name is "event"
                //   -> Set the event type buffer to field value.
                b"event" => {
                    let value = src.decode_utf()?;
                    self.event_type.clear();
                    self.event_type.push_str(value);
                    continue;
                }
                // If the field name is "data"
                // -> If the new data buffer's length is greater than the maximum allowed size,
                //    then then close the decoder
                // -> Otherwise, append the field value to the data buffer,
                //    then append a single U+000A LINE FEED (LF) character to the data buffer.
                b"data" if src.len().saturating_add(1) > self.buf_remaining() => {
                    self.close();
                    return Err(ExceededSizeLimit::new(
                        self.max_buf_len,
                        src.len().saturating_add(1),
                        self.buf_len(),
                    )
                    .into());
                }
                // If the field name is "data"
                // -> Append the field value to the data buffer,
                //    then append a single U+000A LINE FEED (LF) character to the data buffer.
                b"data" => {
                    self.data_buf.put(src);
                    self.data_buf.put_u8(b'\n');
                    continue;
                }
                // If the field name is "id"
                // -> If the field value does not contain U+0000 NULL,
                //    then set the last event ID buffer to the field value.
                // -> Otherwise, ignore the field.
                b"id" => {
                    if src.contains(&b'\0') {
                        let value = String::from_utf8_lossy(src.as_ref());
                        warn!(
                            field = "id",
                            value = value.as_ref(),
                            "ignore invalid value (reason: `id` must not contain null bytes)"
                        );
                        continue;
                    }

                    let value = src.decode_utf()?;
                    self.event_id.clear();
                    self.event_id.push_str(value);
                    continue;
                }
                // If the field name is "retry"
                // -> If the field value consists of only ASCII digits,
                //    then interpret the field value as an integer in base ten,
                //    and set the event stream's reconnection time to that integer.
                // -> Otherwise, ignore the field.
                b"retry" => {
                    // ! SAFETY
                    // u64::parse will not panic if there's invalid utf-8 since
                    // all valid digits are ascii characters.
                    // the standard library calls as_bytes on the input anyway
                    // TODO: should we trim trailing whitespace?
                    let value = unsafe { std::str::from_utf8_unchecked(src.as_ref()) };

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
                        continue;
                    } else {
                        break Ok(retry);
                    }
                }
                // Otherwise, the field is ignored
                _ => {
                    let field = String::from_utf8_lossy(src.as_ref());
                    warn!(field = field.as_ref(), "ignoring unknown field");
                    continue;
                }
            };
        };
    }

    #[instrument(skip(self, buf), err)]
    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Frame>, SSEDecodeError> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(SSEDecodeError::UnexpectedEof)
                }
            }
        }
    }
}

impl Default for SSEDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use futures::StreamExt;
    use tokio_util::codec::FramedRead;

    #[tokio::test]
    async fn empty_lines() {
        let mut src = BytesMut::from(b"event: foo\ndata: bar\n\nhi".as_ref());
        let mut decoder = SSEDecoder::default();
        let (first, second, third, fourth) = (
            decoder.decode_line(&mut src),
            decoder.decode_line(&mut src),
            decoder.decode_line(&mut src),
            decoder.decode_line(&mut src),
        );

        assert!(
            matches!(first, Ok(Some(v)) if v.as_ref() == b"event: foo"),
            "event line"
        );
        assert!(
            matches!(second, Ok(Some(v)) if v.as_ref() == b"data: bar"),
            "data line"
        );
        assert!(matches!(third, Ok(Some(v)) if v.is_empty()), "empty line");
        assert!(matches!(fourth, Ok(None)), "unfinished line");
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
        assert_eq!(decoder.event_id, "1");
        assert!(matches!(event, Frame::Event(Event { id: Some(v), .. }) if v == "1"));
    }
    #[tokio::test]
    async fn require_new_line() {
        let bytes = b"event: foo\ndata: bar";
        let mut framed = FramedRead::new(&bytes[..], SSEDecoder::default());
        let event = framed.next().await.unwrap();
        assert!(matches!(event, Err(SSEDecodeError::UnexpectedEof)));
    }
}
