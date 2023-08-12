use std::fmt::{Display, Formatter};

use thiserror::Error;

use super::{
    consts::{CLIENT_SIDE_ID_LEN, MOBILE_KEY_LEN, RELAY_AUTO_CONFIG_KEY_LEN, SERVER_SIDE_KEY_LEN},
    CredentialKind,
};

#[derive(Error, Debug)]
pub enum CredentialError {
    #[error("invalid {expected:?} length (expected: {0:?}  found: {found:?})", .expected.expected_sizes_message())]
    InvalidLength {
        expected: ExpectedCredential,
        found: usize,
    },

    #[error("invalid credential prefix (expected: {expected:?}, found: {found:?})")]
    InvalidPrefix {
        expected: &'static str,
        found: Option<String>,
    },

    #[error("invalid utf8")]
    InvalidUtf8(#[from] std::str::Utf8Error),

    #[error("invalid format for {expected:?}. reason: {reason}")]
    InvalidFormat {
        expected: ExpectedCredential,
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedCredentialSize {
    ForAny,
    ForKind(CredentialKind),
}

impl Display for ExpectedCredentialSize {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        assert!(
            SERVER_SIDE_KEY_LEN == MOBILE_KEY_LEN
                && SERVER_SIDE_KEY_LEN == RELAY_AUTO_CONFIG_KEY_LEN
        );
        match self {
            ExpectedCredentialSize::ForAny => write!(
                f,
                "one of {} (server-side,mobile-key,relay-auto-config), {} (client-side)",
                SERVER_SIDE_KEY_LEN, CLIENT_SIDE_ID_LEN
            ),
            ExpectedCredentialSize::ForKind(kind) => write!(f, "{}", kind.len()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedCredential {
    Any,
    Kind(CredentialKind),
}

impl ExpectedCredential {
    fn expected_sizes_message(&self) -> String {
        const KINDS: &[CredentialKind; 3] = &[
            CredentialKind::ServerSide,
            CredentialKind::MobileKey,
            CredentialKind::RelayAutoConfig,
        ];
        match self {
            ExpectedCredential::Any => format!(
                "one of {} ({:?}), {:?} {}",
                SERVER_SIDE_KEY_LEN,
                KINDS,
                CredentialKind::ClientSide,
                CLIENT_SIDE_ID_LEN
            ),
            ExpectedCredential::Kind(kind) => format!("{}", kind.len()),
        }
    }
}
impl Display for ExpectedCredential {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpectedCredential::Any => write!(f, "launchdarkly credential"),
            ExpectedCredential::Kind(kind) => write!(f, "{}", kind),
        }
    }
}

impl From<CredentialKind> for ExpectedCredential {
    fn from(kind: CredentialKind) -> Self {
        ExpectedCredential::Kind(kind)
    }
}
