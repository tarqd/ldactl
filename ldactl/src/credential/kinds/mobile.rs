use serde::{Deserialize, Serialize};

use crate::credential::{
    error::CredentialError, CredentialKind, HasConstKind, LaunchDarklyCredential,
    LaunchDarklyCredentialExt,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MobileKey(String);

impl HasConstKind for MobileKey {
    const KIND: CredentialKind = CredentialKind::MobileKey;
}
impl LaunchDarklyCredential for MobileKey {
    fn kind(&self) -> CredentialKind {
        Self::KIND
    }
}
impl LaunchDarklyCredentialExt for MobileKey {
    type Inner = String;

    unsafe fn from_inner_unchecked(s: Self::Inner) -> Self {
        Self(s)
    }
}

impl AsRef<str> for MobileKey {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for MobileKey {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl TryFrom<&[u8]> for MobileKey {
    type Error = CredentialError;
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_bytes(b)
    }
}

impl TryFrom<&str> for MobileKey {
    type Error = CredentialError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from_str(s)
    }
}
impl TryFrom<String> for MobileKey {
    type Error = CredentialError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from_string(s)
    }
}

impl std::fmt::Display for MobileKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
