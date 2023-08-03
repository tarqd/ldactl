use serde::{Deserialize, Serialize};

use crate::credential::{
    error::CredentialError, CredentialKind, HasConstKind, LaunchDarklyCredential,
    LaunchDarklyCredentialExt,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelayAutoConfigKey(String);

impl HasConstKind for RelayAutoConfigKey {
    const KIND: CredentialKind = CredentialKind::RelayAutoConfig;
}

impl LaunchDarklyCredential for RelayAutoConfigKey {
    fn kind(&self) -> CredentialKind {
        Self::KIND
    }
}
impl LaunchDarklyCredentialExt for RelayAutoConfigKey {
    type Inner = String;

    unsafe fn from_inner_unchecked(s: Self::Inner) -> Self {
        Self(s)
    }
}

impl AsRef<str> for RelayAutoConfigKey {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for RelayAutoConfigKey {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl TryFrom<&[u8]> for RelayAutoConfigKey {
    type Error = CredentialError;
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_bytes(b)
    }
}

impl TryFrom<&str> for RelayAutoConfigKey {
    type Error = CredentialError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from_str(s)
    }
}
impl TryFrom<String> for RelayAutoConfigKey {
    type Error = CredentialError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from_string(s)
    }
}

impl std::fmt::Display for RelayAutoConfigKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rel-xxxxxxxx-xxxx-xxxx-xxxx-xxxxxx{}",
            self.0.get(self.0.len() - 6..).unwrap_or("xxxxxx")
        )
    }
}
