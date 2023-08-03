use crate::credential::util::{validate_credential_uuid, validate_uuid_format};

use super::{error::CredentialError, CredentialKind};

pub trait LaunchDarklyCredential: Sized + AsRef<str> + AsRef<[u8]> + TryFrom<String> {
    fn kind(&self) -> CredentialKind;

    fn as_str(&self) -> &str {
        self.as_ref()
    }
    fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }

    fn into_string(self) -> String {
        self.as_str().into()
    }
}

pub trait HasConstKind {
    const KIND: CredentialKind;
}

pub trait LaunchDarklyCredentialExt: LaunchDarklyCredential + HasConstKind {
    type Inner: From<String>;
    unsafe fn from_inner_unchecked(s: Self::Inner) -> Self;

    fn try_validate(b: &[u8]) -> Result<(), CredentialError> {
        if b.len() != Self::KIND.len() {
            Err(CredentialError::InvalidLength {
                expected: Self::KIND.into(),
                found: b.len(),
            })
        } else if let Some(prefix) = Self::KIND.prefix() {
            debug_assert!(
                Self::KIND != CredentialKind::ClientSide,
                "client-side ids should not have a prefix"
            );

            b.strip_prefix(prefix.as_bytes())
                .ok_or_else(|| CredentialError::InvalidPrefix {
                    expected: prefix,
                    found: b
                        .get(0..prefix.len())
                        .map(String::from_utf8_lossy)
                        .map(String::from),
                })
                .and_then(|b| validate_uuid_format(Self::KIND, b))
        } else {
            // client-side ID doesn't have a prefix
            // so we just check the length and characters
            debug_assert!(Self::KIND == CredentialKind::ClientSide);
            if b.len() != Self::KIND.len() {
                Err(CredentialError::InvalidLength {
                    expected: Self::KIND.into(),
                    found: b.len(),
                })
            } else if b.iter().all(u8::is_ascii_hexdigit) {
                Ok(())
            } else {
                Err(CredentialError::InvalidFormat {
                    expected: Self::KIND.into(),
                    reason: "unexpected non-hex character".into(),
                })
            }
        }
    }
    fn try_from_bytes(s: &[u8]) -> Result<Self, CredentialError> {
        // safe because try_validate ensures only ascii characters are present
        // avoids extra allocation by validating first
        Self::try_validate(s)?;
        Ok(unsafe { Self::from_inner_unchecked(String::from_utf8_unchecked(s.to_vec()).into()) })
    }

    fn try_from_str(s: &str) -> Result<Self, CredentialError> {
        Self::try_from_bytes(s.as_bytes())
    }

    fn try_from_string(s: String) -> Result<Self, CredentialError> {
        Self::try_validate(s.as_bytes())?;
        // Same as try_from_str
        Ok(unsafe { Self::from_inner_unchecked(s.into()) })
    }
}
