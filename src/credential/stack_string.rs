use serde::{Serialize, Deserialize};

use super::error::CredentialError;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StackString<const N: usize> {
    buf: [u8; N],
}

impl<const N: usize> StackString<N> {
    pub fn new() -> Self {
        Self { buf: [0; N] }
    }
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
    pub fn into_string(self) -> String {
        self.as_str().to_string()
    }

    pub unsafe fn try_from_utf8_unchecked(s: &[u8]) -> Result<Self, CredentialError> {
        if s.len() < N {
            return Err(CredentialError::InvalidBufLength {
                expected: N,
                found: s.len(),
            });
        }
        let mut buf = [0u8; N];
        buf.clone_from_slice(&s[0..N]);
        Ok(Self { buf })
    }
}

impl<const N: usize> TryFrom<&str> for StackString<N> {
    type Error = CredentialError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut buf = [0u8; N];
        if value.len() > N {
            return Err(CredentialError::InvalidBufLength {
                expected: N,
                found: value.len(),
            });
        }
        buf.clone_from_slice(&value.as_bytes()[0..N]);
        Ok(Self { buf })
    }
}



impl<const N: usize> TryFrom<&[u8; N]> for StackString<N> {
    type Error = CredentialError;
    fn try_from(value: &[u8; N]) -> Result<Self, Self::Error> {
        let mut buf = [0u8; N];
        let s = std::str::from_utf8(value)?;
        buf.clone_from_slice(s.as_bytes());
        Ok(Self { buf })
    }
}

impl<const N: usize> AsRef<str> for StackString<N> {
    fn as_ref(&self) -> &str {
        // We can only construct this type from a valid utf8 string
        unsafe { std::str::from_utf8_unchecked(&self.buf) }
    }
}

impl<const N: usize> AsRef<[u8]> for StackString<N> {
    fn as_ref(&self) -> &[u8] {
        &self.buf
    }
}