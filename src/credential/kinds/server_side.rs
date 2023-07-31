use serde::{Deserialize, Serialize};

use crate::credential::{
    error::CredentialError, CredentialKind, HasConstKind, LaunchDarklyCredential,
    LaunchDarklyCredentialExt,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerSideKey(String);

impl HasConstKind for ServerSideKey {
    const KIND: CredentialKind = CredentialKind::ServerSide;
}
impl LaunchDarklyCredential for ServerSideKey {
    fn kind(&self) -> CredentialKind {
        Self::KIND
    }
}
impl LaunchDarklyCredentialExt for ServerSideKey {
    type Inner = String;

    unsafe fn from_inner_unchecked(s: Self::Inner) -> Self {
        Self(s)
    }
}

impl AsRef<str> for ServerSideKey {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for ServerSideKey {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl TryFrom<&[u8]> for ServerSideKey {
    type Error = CredentialError;
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_bytes(b)
    }
}

impl TryFrom<&str> for ServerSideKey {
    type Error = CredentialError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from_str(s)
    }
}
impl TryFrom<String> for ServerSideKey {
    type Error = CredentialError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from_string(s)
    }
}

impl std::fmt::Display for ServerSideKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
