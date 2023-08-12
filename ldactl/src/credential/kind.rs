use super::consts::{
    CLIENT_SIDE_ID_LEN, MOBILE_KEY_LEN, RELAY_AUTO_CONFIG_KEY_LEN, SERVER_SIDE_KEY_LEN,
};
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CredentialKind {
    ServerSide,
    MobileKey,
    ClientSide,
    RelayAutoConfig,
}

impl std::fmt::Debug for CredentialKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_slug())
    }
}

impl Display for CredentialKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialKind::ServerSide => write!(f, "SDK Key"),
            CredentialKind::MobileKey => write!(f, "Mobile Key"),
            CredentialKind::ClientSide => write!(f, "Client-side Id"),
            CredentialKind::RelayAutoConfig => write!(f, "Relay AutoConfig Key"),
        }
    }
}

impl CredentialKind {
    #[inline]
    fn as_slug(&self) -> &'static str {
        match self {
            CredentialKind::ServerSide => "sdk-key",
            CredentialKind::MobileKey => "mobile-key",
            CredentialKind::ClientSide => "client-side-id",
            CredentialKind::RelayAutoConfig => "relay-auto-config-key",
        }
    }
    #[inline]
    pub const fn len(&self) -> usize {
        match self {
            CredentialKind::ServerSide => SERVER_SIDE_KEY_LEN,
            CredentialKind::MobileKey => MOBILE_KEY_LEN,
            CredentialKind::RelayAutoConfig => RELAY_AUTO_CONFIG_KEY_LEN,
            CredentialKind::ClientSide => CLIENT_SIDE_ID_LEN,
        }
    }

    #[inline]
    pub const fn prefix(&self) -> Option<&'static str> {
        match self {
            CredentialKind::ServerSide => Some("sdk-"),
            CredentialKind::MobileKey => Some("mob-"),
            CredentialKind::RelayAutoConfig => Some("rel-"),
            CredentialKind::ClientSide => None,
        }
    }
}
