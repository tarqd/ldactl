use crate::{BytesStr, DecodeUtf8Error, Event, Frame, SseDecodeError};
use bytes::Bytes;
use std::convert::Infallible;

/// Convert `Frame<Bytes>` into `Frame<T>`
///
/// You can implement this trait for any `Frame<T>` where `T` is a type that can be fallibly constructed from `Bytes`.
/// In order to use with the decoder, `From<Error> for SseDecodeError` must be implemented.
pub trait TryFromBytesFrame
where
    Self: sealed::SseFrame + Sized,
{
    /// Error returned by `try_from_frame`. Should be convertible to `SseDecodeError`
    type Error;
    /// Convert `Frame<Bytes>` into `Frame<T>`
    fn try_from_frame(
        frame: Frame<Bytes>,
    ) -> Result<Frame<<Self as sealed::SseFrame>::Data>, Self::Error>;
}

impl TryFromBytesFrame for Frame<String> {
    type Error = SseDecodeError;
    fn try_from_frame(frame: Frame<Bytes>) -> Result<Self, Self::Error> {
        match frame {
            Frame::Event(Event { id, name, data }) => Ok(Frame::Event(Event {
                id,
                name,
                data: String::from_utf8(data.to_vec())?,
            })),
            Frame::Retry(duration) => Ok(Frame::Retry(duration)),
            Frame::Comment(comment) => Ok(Frame::Comment(String::from_utf8(comment.to_vec())?)),
        }
    }
}

impl TryFromBytesFrame for Frame<BytesStr> {
    type Error = DecodeUtf8Error;
    fn try_from_frame(frame: Frame<Bytes>) -> Result<Self, Self::Error> {
        match frame {
            Frame::Event(Event { id, name, data }) => Ok(Frame::Event(Event {
                id,
                name,
                data: BytesStr::try_from_utf8_bytes(data)?,
            })),
            Frame::Retry(duration) => Ok(Frame::Retry(duration)),
            Frame::Comment(comment) => Ok(Frame::Comment(BytesStr::try_from_utf8_bytes(comment)?)),
        }
    }
}
impl TryFromBytesFrame for Frame<Bytes> {
    type Error = Infallible;
    fn try_from_frame(frame: Frame<Bytes>) -> Result<Self, Self::Error> {
        Ok(frame)
    }
}

/// Automatically implemented for `TryFromBytesFrame<T>`
/// You should not implement this trait yourself!
pub trait TryIntoFrame<T>
where
    T: sealed::SseFrame,
{
    /// Error returned from `try_into_frame`. Should be convertible to `SseDecodeError`
    type Error;
    /// Converts `Frame<Bytes>` into `Frame<T>`
    fn try_into_frame(self) -> Result<Frame<T::Data>, Self::Error>;
}

impl<T> TryIntoFrame<T> for Frame<Bytes>
where
    T: sealed::SseFrame + TryFromBytesFrame,
{
    type Error = <T as TryFromBytesFrame>::Error;
    fn try_into_frame(self) -> Result<Frame<T::Data>, Self::Error> {
        T::try_from_frame(self)
    }
}

impl From<Infallible> for SseDecodeError {
    fn from(_: Infallible) -> Self {
        unreachable!()
    }
}

mod sealed {

    pub trait SseFrame {
        type Data;
    }
    impl<T> SseFrame for crate::Frame<T> {
        type Data = T;
    }
}
