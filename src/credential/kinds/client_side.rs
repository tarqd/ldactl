use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::credential::{
    error::CredentialError, CredentialKind, HasConstKind, LaunchDarklyCredential,
    LaunchDarklyCredentialExt,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientSideId(String);
impl Display for ClientSideId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl HasConstKind for ClientSideId {
    const KIND: CredentialKind = CredentialKind::ClientSide;
}
impl LaunchDarklyCredential for ClientSideId {
    fn kind(&self) -> CredentialKind {
        Self::KIND
    }
}
impl LaunchDarklyCredentialExt for ClientSideId {
    type Inner = String;

    unsafe fn from_inner_unchecked(s: Self::Inner) -> Self {
        Self(s)
    }
}

impl AsRef<str> for ClientSideId {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl AsRef<[u8]> for ClientSideId {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}
impl TryFrom<&[u8]> for ClientSideId {
    type Error = CredentialError;
    fn try_from(b: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_bytes(b)
    }
}

impl TryFrom<&str> for ClientSideId {
    type Error = CredentialError;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::try_from_str(s)
    }
}
impl TryFrom<String> for ClientSideId {
    type Error = CredentialError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::try_from_string(s)
    }
}

#[cfg(test)]
mod tests {
    use crate::credential::error::ExpectedCredential;

    use super::*;
    #[test]
    fn parse_valid() {
        let src = "62ea8c4afac9b011945f6792";
        let ret = ClientSideId::try_from_bytes(src.as_bytes());
        match ret {
            Ok(id) => assert_eq!(src, id.as_str()),
            Err(e) => panic!("unexpected error: {:?}", e),
        }
    }
    #[test]
    fn too_small() {
        let s = b"62ea8c4afac9b011945f679";
        let ret = ClientSideId::try_from_bytes(s);
        match ret {
            Err(CredentialError::InvalidLength {
                expected: ExpectedCredential::Kind(kind),
                found,
            }) => {
                assert_eq!(
                    kind,
                    CredentialKind::ClientSide,
                    "error expected kind should match input kind"
                );
                assert_eq!(found, s.len(), "expected found={}, got={}", s.len(), found);
            }
            _ => panic!("unexpected result: {:?}", ret),
        }
    }
    #[test]
    fn too_big() {
        let s = b"62ea8c4afac9b011945f6792a";
        let ret = ClientSideId::try_from_bytes(s);
        match ret {
            Err(CredentialError::InvalidLength {
                expected: ExpectedCredential::Kind(kind),
                found,
            }) => {
                assert_eq!(
                    kind,
                    CredentialKind::ClientSide,
                    "error expected kind should match input kind"
                );
                assert_eq!(found, s.len(), "expected found={}, got={}", s.len(), found);
            }
            _ => panic!("unexpected result: {:?}", ret),
        }
    }
    #[test]
    fn invalid_chars() {
        let s = b"62ea8c4afac9b011945f679g";
        let ret = ClientSideId::try_from_bytes(s);
        match ret {
            Err(CredentialError::InvalidFormat {
                expected: ExpectedCredential::Kind(kind),
                reason,
            }) => {
                assert_eq!(
                    kind,
                    CredentialKind::ClientSide,
                    "error expected kind should match input kind"
                );
                assert_eq!(
                    reason, "unexpected non-hex character",
                    "expected reason='{}', got='{}'",
                    "unexpected non-hex character", reason
                );
            }
            _ => panic!("unexpected result: {:?}", ret),
        }
    }
}
