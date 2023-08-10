//! # Error Extensions
//!
//! This trait is used by [`super::eventsource::EventSource`] to determine how an error
//! should be handled. It extracts inner errors that get lost through stream combinators
//! For example, when we pass reqwests response stream through a framed codec, everything
//! gets wrapped in a `std::io::Error`.
//!
//! It should be implemented for any error that can occur while processing the stream.
//!
use super::eventsource::EventSourceError;
use reqwest::Error as ReqwestError;
use std::error::Error;
use std::io::Error as IOError;
use tokio_sse_codec::SseDecodeError;

pub trait EventSourceErrorInnerError: Error + Into<EventSourceError> + private::Sealed {
    #[inline]
    fn into_event_source_error(self) -> EventSourceError {
        self.into()
    }
}
// identity impl
impl EventSourceErrorInnerError for EventSourceError {
    #[inline]
    fn into_event_source_error(self) -> EventSourceError {
        self
    }
}
// trival Into/From impls
impl EventSourceErrorInnerError for SseDecodeError {}
impl EventSourceErrorInnerError for ReqwestError {}

// Downcast IO errors if the inner error is an eventsource error
// Otherwise, just bubble it back up as EventSourceError::IO
impl EventSourceErrorInnerError for IOError {
    fn into_event_source_error(self) -> EventSourceError {
        // can be replaced with std::error::Error::downcast when it's stable
        match self.get_ref() {
            Some(e) if e.is::<EventSourceError>() => *self
                .into_inner()
                .expect("std::io::Error::into_inner failed unexpectly. This should never happen")
                .downcast::<EventSourceError>()
                .expect("downcast<EventSourceError> failed unexpectedly. This should never happen"),
            _ => self.into(),
        }
    }
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::EventSourceError {}
    impl Sealed for super::ReqwestError {}
    impl Sealed for super::IOError {}
    impl Sealed for super::SseDecodeError {}
}
