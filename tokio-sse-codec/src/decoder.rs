#![deny(missing_docs)]
#![allow(warnings)]
use crate::{decoder_impl::SseDecoderImpl, errors::SseDecodeError, Frame, TryIntoFrame};
use bytes::{Bytes, BytesMut};
use std::{borrow::Cow, marker::PhantomData};
use tokio_util::codec::Decoder;

/// Decodes bytes from an SSE Stream into [`Frame<T>`]
///  
/// The `T` type parameter represents the type used to store event and comment data in [`Frame<T>`].
///
///
/// There are 4 default implementations:
/// - `Frame<String>`: This is the default type used by [`SseDecoder`]. Easy to use, but may copy if the underlying buffer is still shared.
/// - `Frame<Cow<'static, str>>`: Effectively the same as `Frame<String>` but will avoid allocating for common event types (right now just `message`) and empty comments/events
/// - `Frame<Bytes>`: Returns a zero-copy slice of the underlying buffer. UTF-8 validity is not checked. This is cheaply cloneable but maintains a reference the underlying shared vector. Use it and drop it quickly to avoid wasting memory`
/// - `Frame<BytesStr>`: A zero-copy "string" reference backed by bytes. Same as `FrameBytes` but validates UTF-8 and implements `Deref<str>` for convienence.
///
/// ## Quick Links
/// - [`SseDecodeError`]: Type for unrecoverable decoder errors
/// - [`Frame`]: Type representing a parsed frame returned by [`SseDecoder::decode`]
/// - [`BytesStr`]: Wrapper around `Bytes` used for zero-copy access to guaranteed valid utf-8 data
///
/// ## Example
///
/// ```rust
/// use bytes::BytesMut;
/// use tokio_util::codec::Decoder;
/// use tokio_sse_codec::{SseDecoder, Event, Frame};
///
/// let mut buffer = BytesMut::from("data: hello\n\n");
/// let mut decoder : SseDecoder<String> = SseDecoder::new();
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
/// [`Frame<T>`]: crate::Frame
/// [`BytesStr`]: crate::BytesStr
/// [`TryFromBytesFrame`]: crate::TryFromBytesFrame

pub struct SseDecoder<T = String> {
    inner: SseDecoderImpl,
    phantom: std::marker::PhantomData<T>,
}

/// Tuple representing the internal buffers of the decoder
/// Most users should not use this directly unless you're re-using the buffers
/// after consuming the decoder.
///
/// Tuple contains `(data_buf, max_buf_len)`
pub type DecoderParts = (BytesMut, usize);

impl<T> SseDecoder<T> {
    /// Returns an `SSECodec` with no maximum buffer size limit.
    ///
    /// # Note
    ///
    /// Setting a buffer size limit is highly recommended for any `SSECodec` which
    /// will be exposed to untrusted input. Otherwise, the size of the buffer
    /// that holds event currently being read is unbounded. An attacker could
    /// exploit this unbounded buffer by sending unbounded event names, ids or data fields
    /// , causing unbounded memory consumption.
    ///
    ///
    /// [`SSEDecodeError`]: crate::decoder::SSEDecodeError
    pub fn new() -> Self {
        Self {
            inner: SseDecoderImpl::new(),
            phantom: PhantomData,
        }
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
        Self {
            phantom: PhantomData,
            inner: SseDecoderImpl::with_max_size(max_buf_size),
        }
    }

    /// Returns the internal buffers and state of the decoder as a tuple
    /// This is useful for re-using the buffers when you're done with them
    /// See [`DecoderParts`]
    pub fn into_parts(self) -> DecoderParts {
        self.inner.into_parts()
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
        Self {
            phantom: PhantomData,
            inner: SseDecoderImpl::from_parts(parts),
        }
    }

    /// Returns the current value of the event type buffer
    /// This value is set by when `event` field is received
    /// It is cleared when an event is dispatched
    /// Defaults to `message` if not set
    pub fn current_event_type(&self) -> &Cow<'static, str> {
        self.inner.current_event_type()
    }
    /// Returns the maximum buffer size when decoding.
    pub fn max_buf_size(&self) -> usize {
        self.inner.max_buf_size()
    }

    /// Returns true if the decoder has been closed due to permanent error such
    /// as the buffer capacity being exceeded.
    ///
    /// When the decoder is closed, any further writes will be dropped and decode will return `None`
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Resets the decoder to a state where it can decode events after closing
    /// Calling this method is the equivalent of `let decoder = SseDecoder::from_parts(unsafe { decoder.into_parts() })`
    ///
    /// The difference is that this method does not consume `self` and you don't need to worry about constructing an invalid decoder
    pub fn reset(&mut self) {
        self.inner.reset()
    }
}

impl<T> Decoder for SseDecoder<T>
where
    Frame<Bytes>: TryIntoFrame<Frame<T>>,
    <Frame<Bytes> as TryIntoFrame<Frame<T>>>::Error: Into<SseDecodeError>,
{
    type Item = Frame<T>;

    type Error = SseDecodeError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.inner.decode(src)? {
            Ok(Some(frame.try_into_frame().map_err(Into::into)?))
        } else {
            Ok(None)
        }
    }
    fn decode_eof(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(frame) = self.inner.decode_eof(src)? {
            Ok(Some(frame.try_into_frame().map_err(Into::into)?))
        } else {
            Ok(None)
        }
    }
}

impl<T> Default for SseDecoder<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {

    use crate::{Event, TryFromBytesFrame};

    use super::*;
    //use futures::StreamExt;
    use tokio_util::codec::FramedRead;
    type SseDecoder = super::SseDecoder<Bytes>;
    use bytes::{BufMut, BytesMut};

    #[test]
    fn test_event() {
        let mut bytes = BytesMut::from(b"event: foo\ndata: bar\n\n".as_ref());
        let mut decoder = SseDecoder::default();
        let event = decoder.decode(&mut bytes).unwrap().unwrap();

        // should reset after event dispatch
        assert_eq!(decoder.current_event_type(), "message");
        assert_eq!(
            event,
            Frame::Event(Event {
                id: None,
                name: "foo".into(),
                data: "bar".into()
            })
        );
    }
    #[test]
    fn test_current_event_type() {
        let mut bytes = BytesMut::from(b"event: foo\ndata: bar\nevent: baz\n".as_ref());
        let mut decoder = SseDecoder::default();
        let _ = decoder.decode(&mut bytes);

        assert_eq!(decoder.current_event_type(), "baz");
    }

    #[test]
    fn test_event_retry() {
        let mut bytes = BytesMut::from(b"retry: 100\n".as_ref());
        let mut decoder = SseDecoder::default();
        let event = decoder.decode(&mut bytes).unwrap().unwrap();

        assert_eq!(event, Frame::Retry(std::time::Duration::from_millis(100)));
    }
    #[test]
    fn test_event_retry_invalid() {
        let mut bytes = BytesMut::from(b"retry: foo\n".as_ref());
        let mut decoder = SseDecoder::default();
        let event = decoder.decode(&mut bytes).unwrap();

        assert!(event.is_none());
    }

    #[test]
    fn event_has_id() {
        let mut bytes = BytesMut::from(b"id: 1\nevent: foo\ndata: bar\n\n".as_ref());
        let mut decoder = SseDecoder::default();
        let event = decoder.decode(&mut bytes).unwrap().unwrap();

        assert!(matches!(event, Frame::Event(Event { id: Some(v), .. }) if v.as_bytes() == b"1"));
    }
    #[test]
    fn require_blank_line() {
        let mut bytes = BytesMut::from(b"event: foo\ndata: bar".as_ref());
        let mut decoder = SseDecoder::default();
        let event = decoder.decode(&mut bytes);
        assert!(matches!(event, Ok(None)));
        bytes.put_u8(b'\n');
        // should error if we reported this the last write
        let event = decoder.decode_eof(&mut bytes);
        assert!(matches!(event, Err(SseDecodeError::UnexpectedEof)));
    }
}
