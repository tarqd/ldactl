use super::{error::CredentialError, CredentialKind};

pub fn validate_credential_uuid(kind: CredentialKind, s: &[u8]) -> Result<(), CredentialError> {
    let prefix = kind
        .prefix()
        .expect("Only prefixed kinds can be used with validate_prefix_key");

    let len = prefix.as_bytes().len();
    if len != kind.len() {
        return Err(CredentialError::InvalidLength {
            expected: kind.into(),
            found: len,
        });
    }
    match s.get(..len) {
        Some(p) if p == prefix.as_bytes() => Ok(()),
        Some(p) => Err(CredentialError::InvalidPrefix {
            expected: prefix,
            found: Some(String::from_utf8_lossy(p).into()),
        }),
        None => Err(CredentialError::InvalidLength {
            expected: kind.into(),
            found: s.len(),
        }),
    }?;
    match s.get(len..) {
        Some(s) => validate_uuid_format(kind, s),
        None => Err(CredentialError::InvalidLength {
            expected: kind.into(),
            found: s.len(),
        }),
    }
}

pub fn validate_uuid_format(kind: CredentialKind, s: &[u8]) -> Result<(), CredentialError> {
    assert_ne!(kind, CredentialKind::ClientSide);

    const UUID_LEN: usize = 36;
    if s.len() != UUID_LEN {
        return Err(CredentialError::InvalidLength {
            expected: kind.into(),
            found: s.len(),
        });
    }
    // XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX
    // Based on https://github.com/uuid-rs/uuid
    // MIT License
    match [s[8], s[13], s[18], s[23]] {
        [b'-', b'-', b'-', b'-'] => {}
        _ => {
            return Err(CredentialError::InvalidFormat {
                expected: kind.into(),
                reason: format!(
                    "key should be in the format of {0}XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX",
                    kind.prefix().unwrap_or("")
                ),
            })
        }
    }
    let parts = &[&s[0..8], &s[9..13], &s[14..18], &s[19..23], &s[24..]];
    debug_assert!(parts.map(|p| p.len()).eq(&[8, 4, 4, 4, 12]));

    if parts
        .iter()
        .flat_map(|p| p.iter())
        .all(u8::is_ascii_hexdigit)
    {
        Ok(())
    } else {
        Err(CredentialError::InvalidFormat {
            expected: kind.into(),
            reason: "unexpected non-hex character in key".into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn can_parse_valid_uuid() {
        let uuid = "01345678-9ABC-DEF0-0000-000000000000";
        let uuid = uuid.as_bytes();
        assert!(validate_uuid_format(CredentialKind::ServerSide, uuid).is_ok());
    }
    #[test]
    fn fails_if_too_small() {
        let uuid = "12345".as_bytes();
        let ret = validate_uuid_format(CredentialKind::ServerSide, uuid);
        assert!(ret.is_err());
        match ret {
            Err(CredentialError::InvalidLength { expected, found }) => {
                assert_eq!(expected, CredentialKind::ServerSide.into());
                assert_eq!(found, uuid.len());
            }
            Err(e) => panic!("expected CredentialError::InvalidLength, got {:?}", e),
            Ok(_) => panic!("expected an error, got Ok(_)"),
        }
    }

    #[test]
    fn fails_if_too_big() {
        let uuid = "01345678-9ABC-DEF0-0000-000000000000000".as_bytes();
        let ret = validate_uuid_format(CredentialKind::ServerSide, uuid);
        assert!(ret.is_err());
        match ret {
            Err(CredentialError::InvalidLength { expected, found }) => {
                assert_eq!(expected, CredentialKind::ServerSide.into());
                assert_eq!(found, uuid.len());
            }
            Err(e) => panic!("expected CredentialError::InvalidLength, got {:?}", e),
            Ok(_) => panic!("expected an error, got Ok(_)"),
        }
    }

    #[test]
    fn fails_if_dashes_wrong() {
        let uuids = [
            "0000000-00000-0000-0000-000000000000",
            "00000000-000-00000-0000-000000000000",
            "00000000-0000-000-00000-000000000000",
            "00000000-0000-0000-000000-0000000000",
            "00000000-0000-0000-000000000-0000000",
        ]
        .iter();
        for uuid in uuids {
            assert!(validate_uuid_format(CredentialKind::ServerSide, uuid.as_bytes()).is_err());
        }
    }

    #[test]
    fn fails_if_not_hex() {
        let uuid = "00U00000-0000-0000-0000-000000000000";
        let ret = validate_uuid_format(CredentialKind::ServerSide, uuid.as_bytes());
        assert!(ret.is_err());
    }
}
