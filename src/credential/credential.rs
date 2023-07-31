use serde::{Deserialize, Serialize};

use crate::credential::{
    error::CredentialError, kind::CredentialKind, ClientSideId, MobileKey, RelayAutoConfigKey,
    ServerSideKey,
};

use super::{error::ExpectedCredential, LaunchDarklyCredential};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Credential {
    Server(ServerSideKey),
    Mobile(MobileKey),
    Client(ClientSideId),
    RelayAutoConfig(RelayAutoConfigKey),
}

impl LaunchDarklyCredential for Credential {
    #[inline]
    fn kind(&self) -> CredentialKind {
        match &self {
            Credential::Server(_) => CredentialKind::ServerSide,
            Credential::Mobile(_) => CredentialKind::MobileKey,
            Credential::Client(_) => CredentialKind::ClientSide,
            Credential::RelayAutoConfig(_) => CredentialKind::RelayAutoConfig,
        }
    }

    #[inline]
    fn as_str(&self) -> &str {
        match &self {
            Credential::Server(s) => s.as_str(),
            Credential::Mobile(m) => m.as_str(),
            Credential::Client(c) => c.as_str(),
            Credential::RelayAutoConfig(r) => r.as_str(),
        }
    }
}
fn try_parse_kind(b: &[u8]) -> Result<CredentialKind, CredentialError> {
    match b.get(0..4) {
        Some(b"sdk-") => Ok(CredentialKind::ServerSide),
        Some(b"mob-") => Ok(CredentialKind::MobileKey),
        Some(b"rel-") => Ok(CredentialKind::RelayAutoConfig),
        Some(_) => Ok(CredentialKind::ClientSide),
        _ => Err(CredentialError::InvalidLength {
            expected: ExpectedCredential::Any,
            found: b.len(),
        }),
    }
}
impl TryFrom<String> for Credential {
    type Error = CredentialError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        let kind = try_parse_kind(s.as_bytes())?;
        match kind {
            CredentialKind::ServerSide => Ok(Self::Server(ServerSideKey::try_from(s)?)),
            CredentialKind::MobileKey => Ok(Self::Mobile(MobileKey::try_from(s)?)),
            CredentialKind::ClientSide => Ok(Self::Client(ClientSideId::try_from(s)?)),
            CredentialKind::RelayAutoConfig => {
                Ok(Self::RelayAutoConfig(RelayAutoConfigKey::try_from(s)?))
            }
        }
    }
}
impl AsRef<str> for Credential {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for Credential {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        match &self {
            Credential::Server(s) => s.as_ref(),
            Credential::Mobile(m) => m.as_ref(),
            Credential::Client(c) => c.as_ref(),
            Credential::RelayAutoConfig(r) => r.as_ref(),
        }
    }
}
