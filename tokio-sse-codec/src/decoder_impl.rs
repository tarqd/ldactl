use bytes::{BufMut, Bytes, BytesMut};
use std::borrow::Cow;
use tokio_util::codec::Decoder;
use tracing::warn;

use crate::{
    bufext::{BufExt, BufMutExt},
    errors::{ExceededSizeLimitError, SseDecodeError},
    field_decoder::{FieldFrame, FieldKind, SseFieldDecoder as FieldDecoder},
    DecodeUtf8Error, DecoderParts, Event, Frame,
};

// Optimizations for common event types
static MESSAGE_EVENT: &str = "message";
static EMPTY_ID: &str = "";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SseDecoderImpl {
    field_decoder: FieldDecoder,
    data_buf: BytesMut,
    event_type: Cow<'static, str>,
    event_id: Cow<'static, str>,
    max_buf_len: usize,
    is_closed: bool,
}

impl SseDecoderImpl {
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
    /// [`ExceededSizeLimit`] error if the event buffers reach this size before dispatching
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
    /// [`ExceededSizeLimit`]: crate::decoder::SseDecodeError::ExceededSizeLimit
    pub fn with_max_size(max_buf_size: usize) -> Self {
        debug_assert!(
            max_buf_size > 7,
            "max_buf_size must be greater than 7 to parse any valid SSE frame"
        );
        Self {
            field_decoder: FieldDecoder::with_max_buf_size(max_buf_size),
            data_buf: BytesMut::new(),
            event_type: Cow::Borrowed(MESSAGE_EVENT),
            event_id: Cow::Borrowed(EMPTY_ID),
            max_buf_len: max_buf_size,
            is_closed: false,
        }
    }

    /// Returns the internal buffers and state of the decoder as a tuple
    /// This is useful for re-using the buffers when you're done with them
    /// See [`DecoderParts`]
    pub fn into_parts(self) -> DecoderParts {
        (self.data_buf, self.max_buf_len)
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
        let (data_buf, max_buf_size) = parts;
        Self {
            field_decoder: FieldDecoder::new(),
            data_buf,
            event_type: Cow::Borrowed(MESSAGE_EVENT),
            event_id: Cow::Borrowed(EMPTY_ID),
            max_buf_len: max_buf_size,
            is_closed: false,
        }
    }

    /// Returns the current value of the event type buffer
    /// This value is set by when `event` field is received
    /// It is cleared when an event is dispatched
    /// Defaults to `message` if not set
    pub fn current_event_type(&self) -> &Cow<'static, str> {
        &self.event_type
    }
    /// Returns the maximum buffer size when decoding.
    pub fn max_buf_size(&self) -> usize {
        self.max_buf_len
    }

    pub(crate) fn buf_len(&self) -> usize {
        self.data_buf.len()
            + self.event_id.len()
            + match &self.event_type {
                Cow::Borrowed(_) => 0,
                Cow::Owned(value) => value.len(),
            }
    }

    pub(crate) fn buf_remaining(&self) -> usize {
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
    /// Calling this method is the equivalent of `let decoder = SseDecoder::from_parts(unsafe { decoder.into_parts() })`
    ///
    /// The difference is that this method does not consume `self` and you don't need to worry about constructing an invalid decoder
    pub fn reset(&mut self) {
        self.data_buf.clear();
        self.event_type = Cow::Borrowed(MESSAGE_EVENT);
        self.event_id = Cow::Borrowed(EMPTY_ID);
        self.field_decoder = FieldDecoder::new();
        self.is_closed = false;
    }

    /// Clear internal buffers after closing to allow re-use via [`SseDecoder::into_parts`]
    fn close(&mut self) {
        self.reset();
        self.is_closed = true;
    }
}

// the event source parts
impl SseDecoderImpl {
    pub fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame<Bytes>>, SseDecodeError> {
        if self.is_closed {
            // just consume everything while we're closed
            src.clear();
            return Ok(None);
        }

        while let Some(field) = {
            self.field_decoder.set_consumed(self.buf_len());
            self.field_decoder.decode(src)?
        } {
            match field {
                FieldFrame::Field((field, mut value)) => match field {
                    FieldKind::Data => {
                        if value.len() > self.buf_remaining() {
                            self.close();
                            return Err(SseDecodeError::ExceededSizeLimit(
                                ExceededSizeLimitError::new(
                                    self.max_buf_len,
                                    value.len(),
                                    self.buf_len(),
                                ),
                            ));
                        }
                        // we need to strip the carriage return
                        if value.len() >= 2 && &value[value.len() - 2..] == b"\r\n" {
                            value.truncate(value.len() - 2);
                            self.data_buf.reserve(value.len() + 1);
                            self.data_buf.put(value);
                            self.data_buf.put_u8(b'\n');
                        } else {
                            // we can re-use the new line from the field decoder
                            // no need to reserve since put will call extend_from_slice
                            // which will reserve value.len()
                            self.data_buf.put(value);
                        }
                    }
                    FieldKind::Event => {
                        // trim the new line
                        value.rbump();
                        value.rbump_if(b'\r');

                        if self.event_type.as_bytes() != value.as_ref() {
                            self.event_type = get_event_type(value)?;
                        }
                    }
                    FieldKind::Retry => {
                        // SAFETY: u64::parse will bail if there's any non-ascii digit characters
                        value.rbump();
                        value.rbump_if(b'\r');

                        let value = unsafe { std::str::from_utf8_unchecked(value.as_ref()) };
                        return Ok(value
                            .parse()
                            .ok() // spec says to ignore valid values
                            .map(std::time::Duration::from_millis)
                            .map(Frame::Retry));
                    }
                    FieldKind::Comment => {
                        value.rbump();
                        value.rbump_if(b'\r');

                        return Ok(Some(Frame::Comment(value)));
                    }
                    FieldKind::Id => {
                        value.rbump();
                        value.rbump_if(b'\r');
                        if value.find_byte(b'\0').is_some() {
                            let value = String::from_utf8_lossy(value.as_ref());
                            warn!(
                                field = "id",
                                value = value.as_ref(),
                                "ignore invalid value (reason: `id` must not contain null bytes)"
                            );
                        } else if value != self.event_id.as_bytes() {
                            self.event_id = Cow::Owned(String::from_utf8(value.to_vec())?)
                        }
                    }
                    FieldKind::UnknownField(field_name) => {
                        value.rbump();
                        value.rbump_if(b'\r');
                        let field = String::from_utf8_lossy(field_name.as_ref());
                        let value = String::from_utf8_lossy(value.as_ref());
                        warn!(
                            field = field.as_ref(),
                            value = value.as_ref(),
                            "ignoring unknown sse field"
                        );
                    }
                },
                FieldFrame::EmptyLine => {
                    // dispatch time :)
                    // remove trailing new line
                    self.data_buf.rbump();
                    if self.data_buf.is_empty() {
                        // reset the event type
                        self.event_type = Cow::Borrowed(MESSAGE_EVENT);
                        continue;
                    } else {
                        let id = if self.event_id.is_empty() {
                            None
                        } else {
                            Some(self.event_id.clone())
                        };
                        // reset the message type
                        let name =
                            std::mem::replace(&mut self.event_type, Cow::Borrowed(MESSAGE_EVENT));
                        // and the buffer (split clears it, leaving remaining capacity untouched)
                        let data = self.data_buf.split().freeze();
                        return Ok(Some(Frame::Event(Event { id, name, data })));
                    }
                }
            };
        }
        Ok(None)
    }
    pub fn decode_eof(
        &mut self,
        src: &mut BytesMut,
    ) -> Result<Option<Frame<Bytes>>, SseDecodeError> {
        match self.decode(src)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if src.is_empty() && self.data_buf.is_empty() {
                    Ok(None)
                } else {
                    Err(SseDecodeError::UnexpectedEof)
                }
            }
        }
    }
}

/// Returns a static bytes for known events, otherwise returns `buf`
#[inline(always)]
fn get_event_type(buf: Bytes) -> Result<Cow<'static, str>, DecodeUtf8Error> {
    if buf.as_ref() == MESSAGE_EVENT.as_bytes() {
        Ok(Cow::Borrowed(MESSAGE_EVENT))
    } else {
        Ok(Cow::Owned(String::from_utf8(buf.to_vec())?))
    }
}
