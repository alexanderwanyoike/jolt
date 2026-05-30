use std::fmt;
use std::str::FromStr;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};

use crate::JoltError;

const IDENTITY_PUBLIC_KEY_LEN: usize = 32;
const JOLT_SUFFIX: &str = ".jolt";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdentityId {
    public_key: [u8; IDENTITY_PUBLIC_KEY_LEN],
}

impl IdentityId {
    pub fn from_public_key(public_key: [u8; IDENTITY_PUBLIC_KEY_LEN]) -> Self {
        Self { public_key }
    }

    pub fn as_public_key_bytes(&self) -> &[u8; IDENTITY_PUBLIC_KEY_LEN] {
        &self.public_key
    }
}

impl fmt::Display for IdentityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            BASE32_NOPAD.encode(&self.public_key).to_ascii_lowercase()
        )
    }
}

impl FromStr for IdentityId {
    type Err = JoltError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 63 || s != s.to_ascii_lowercase() {
            return Err(JoltError::InvalidIdentityAddress(
                "identity label must be lowercase base32 and fit in one DNS label".to_string(),
            ));
        }

        let decoded = BASE32_NOPAD
            .decode(s.to_ascii_uppercase().as_bytes())
            .map_err(|e| JoltError::InvalidIdentityAddress(e.to_string()))?;
        let public_key: [u8; IDENTITY_PUBLIC_KEY_LEN] = decoded.try_into().map_err(|_| {
            JoltError::InvalidIdentityAddress("identity must decode to a 32-byte key".to_string())
        })?;

        Ok(Self { public_key })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct JoltAddress {
    identity: IdentityId,
    path: String,
}

impl JoltAddress {
    pub fn new(identity: IdentityId, path: impl AsRef<str>) -> Result<Self, JoltError> {
        Ok(Self {
            identity,
            path: normalize_path(path.as_ref())?,
        })
    }

    pub fn identity(&self) -> &IdentityId {
        &self.identity
    }

    pub fn path(&self) -> &str {
        &self.path
    }
}

impl fmt::Display for JoltAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path == "/" {
            write!(f, "{}{JOLT_SUFFIX}", self.identity)
        } else {
            write!(f, "{}{JOLT_SUFFIX}{}", self.identity, self.path)
        }
    }
}

impl FromStr for JoltAddress {
    type Err = JoltError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim() != s || s.contains('?') || s.contains('#') {
            return Err(JoltError::InvalidIdentityAddress(
                "address must not contain whitespace, query, or fragment".to_string(),
            ));
        }

        let (host, raw_path) = s.split_once('/').unwrap_or((s, ""));
        let identity_label = host.strip_suffix(JOLT_SUFFIX).ok_or_else(|| {
            JoltError::InvalidIdentityAddress("address must end with .jolt".to_string())
        })?;

        if identity_label.is_empty() || identity_label.contains('.') {
            return Err(JoltError::InvalidIdentityAddress(
                "address must contain exactly one identity label before .jolt".to_string(),
            ));
        }

        let identity = IdentityId::from_str(identity_label)?;
        Self::new(identity, raw_path)
    }
}

fn normalize_path(path: &str) -> Result<String, JoltError> {
    if path.is_empty() || path == "/" {
        return Ok("/".to_string());
    }

    if path.contains('?') || path.contains('#') || path.chars().any(char::is_whitespace) {
        return Err(JoltError::InvalidIdentityAddress(
            "path must not contain whitespace, query, or fragment".to_string(),
        ));
    }

    let normalized = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };

    if normalized
        .split('/')
        .any(|segment| segment == "." || segment == "..")
    {
        return Err(JoltError::InvalidIdentityAddress(
            "path must not contain . or .. segments".to_string(),
        ));
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::{IdentityId, JoltAddress};
    use std::str::FromStr;

    fn public_key() -> [u8; 32] {
        [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, 28, 29, 30, 31,
        ]
    }

    #[test]
    fn identity_id_derivation_is_deterministic_for_public_key() {
        let first = IdentityId::from_public_key(public_key());
        let second = IdentityId::from_public_key(public_key());

        assert_eq!(first, second);
        assert_eq!(first.as_public_key_bytes(), &public_key());
    }

    #[test]
    fn identity_address_round_trips_through_parse_and_display() {
        let identity = IdentityId::from_public_key(public_key());
        let address = JoltAddress::new(identity, "/").unwrap();

        let parsed = JoltAddress::from_str(&address.to_string()).unwrap();

        assert_eq!(parsed, address);
        assert!(address.to_string().ends_with(".jolt"));
    }

    #[test]
    fn parses_identity_address_with_path() {
        let identity = IdentityId::from_public_key(public_key());
        let address = format!("{identity}.jolt/profile");

        let parsed = JoltAddress::from_str(&address).unwrap();

        assert_eq!(parsed.identity(), &identity);
        assert_eq!(parsed.path(), "/profile");
    }

    #[test]
    fn empty_or_missing_path_normalizes_to_root() {
        let identity = IdentityId::from_public_key(public_key());

        assert_eq!(
            JoltAddress::from_str(&format!("{identity}.jolt"))
                .unwrap()
                .path(),
            "/"
        );
        assert_eq!(
            JoltAddress::from_str(&format!("{identity}.jolt/"))
                .unwrap()
                .path(),
            "/"
        );
    }

    #[test]
    fn rejects_invalid_identity_addresses() {
        let cases = [
            "alice.example/profile",
            "alice.jolt",
            "abc.def.jolt/profile",
            ".jolt/profile",
            "not even an address",
        ];

        for case in cases {
            assert!(JoltAddress::from_str(case).is_err(), "{case} should fail");
        }
    }

    #[test]
    fn rejects_malformed_paths() {
        let identity = IdentityId::from_public_key(public_key());
        let cases = [
            format!("{identity}.jolt/profile?debug=true"),
            format!("{identity}.jolt/profile#section"),
            format!("{identity}.jolt/posts/../secrets"),
        ];

        for case in cases {
            assert!(JoltAddress::from_str(&case).is_err(), "{case} should fail");
        }
    }
}
