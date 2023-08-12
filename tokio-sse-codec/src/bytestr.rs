use std::borrow::Borrow;

use crate::{bufext::Utf8DecodeDiagnostic, DecodeUtf8Error};

/// Represents a str reference backed by [`bytes::Bytes`]
///
/// This type is used to avoid allocations when parsing lines.
/// The underlying bytes are guaranteed to contain valid utf-8
/// It implements [`std::ops::Deref`] for [`str`] for convenience
///
/// It behaves as a smart pointer, so cloning it is cheap.
#[derive(Default, Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct BytesStr {
    inner: bytes::Bytes,
}
impl BytesStr {
    /// Creates a new instance from [`bytes::Bytes`]
    /// # Safety
    /// The underlying bytes must contain valid utf-8 or the behavior is undefined.
    pub unsafe fn from_utf8_bytes_unchecked(inner: bytes::Bytes) -> Self {
        Self { inner }
    }
    /// Consumes and validates the underlying bytes as utf-8
    pub fn try_from_utf8_bytes(value: bytes::Bytes) -> Result<Self, DecodeUtf8Error> {
        let _ = value.decode_utf()?;
        Ok(Self { inner: value })
    }
    /// Get a reference to the underlying bytes
    pub fn get_ref(this: &Self) -> &bytes::Bytes {
        &this.inner
    }
    /// Consumes a `BytesStr` instance and returns the underlying bytes
    pub fn into_bytes(this: Self) -> bytes::Bytes {
        this.inner
    }
    /// Get a mutable reference to the underlying bytes
    /// Manipulating the underlying bytes is not recommended.
    /// # Safety
    /// The underlying bytes must contain valid utf-8 or the behavior is undefined.
    pub unsafe fn get_mut_unchecked(&mut self) -> &mut bytes::Bytes {
        &mut self.inner
    }
}

impl Borrow<bytes::Bytes> for BytesStr {
    fn borrow(&self) -> &bytes::Bytes {
        &self.inner
    }
}

impl std::ops::Deref for BytesStr {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        // ! SAFETY:
        // The underlying bytes are guaranteed to contain valid utf-8 by the decoder
        // and the type can only constructed by the decoder
        unsafe { std::str::from_utf8_unchecked(self.inner.as_ref()) }
    }
}
