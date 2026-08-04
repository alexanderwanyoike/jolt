use std::fmt;
use std::str::FromStr;

use cid::{Cid, Version};
use multihash_codetable::{Code, MultihashDigest};
use serde::{Deserialize, Serialize};

use crate::error::JoltError;

const RAW_CODEC: u64 = 0x55;
const BLAKE3_256_CODE: u64 = 0x1e;
const BLAKE3_256_DIGEST_LEN: u8 = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ContentId(Cid);

impl ContentId {
    /// Hash bytes and produce a content identifier.
    pub fn from_bytes(data: &[u8]) -> Self {
        let multihash = Code::Blake3_256.digest(data);
        let cid = Cid::new_v1(RAW_CODEC, multihash);
        Self(cid)
    }

    /// Verify that data matches this content identifier.
    pub fn verify(&self, data: &[u8]) -> bool {
        let expected = Self::from_bytes(data);
        self.0 == expected.0
    }

    /// Get the underlying CID.
    pub fn as_cid(&self) -> &Cid {
        &self.0
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ContentId {
    type Err = JoltError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let cid = Cid::try_from(s).map_err(|e| JoltError::InvalidContentId(e.to_string()))?;
        if cid.version() != Version::V1 {
            return Err(JoltError::InvalidContentId(format!(
                "unsupported CID version {:?}, expected CIDv1",
                cid.version()
            )));
        }
        if cid.codec() != RAW_CODEC {
            return Err(JoltError::InvalidContentId(format!(
                "unsupported CID codec 0x{:x}, expected raw (0x55)",
                cid.codec()
            )));
        }
        if cid.hash().code() != BLAKE3_256_CODE {
            return Err(JoltError::InvalidContentId(format!(
                "unsupported multihash 0x{:x}, expected BLAKE3-256 (0x1e)",
                cid.hash().code()
            )));
        }
        if cid.hash().size() != BLAKE3_256_DIGEST_LEN {
            return Err(JoltError::InvalidContentId(format!(
                "invalid multihash digest length {}, expected 32",
                cid.hash().size()
            )));
        }
        Ok(Self(cid))
    }
}

impl Serialize for ContentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ContentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_is_deterministic() {
        let data = b"hello jolt";
        let id1 = ContentId::from_bytes(data);
        let id2 = ContentId::from_bytes(data);
        assert_eq!(id1, id2);
    }

    #[test]
    fn different_data_produces_different_ids() {
        let id1 = ContentId::from_bytes(b"hello");
        let id2 = ContentId::from_bytes(b"world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn verify_returns_true_for_matching_data() {
        let data = b"test content";
        let id = ContentId::from_bytes(data);
        assert!(id.verify(data));
    }

    #[test]
    fn verify_returns_false_for_tampered_data() {
        let data = b"test content";
        let id = ContentId::from_bytes(data);
        assert!(!id.verify(b"tampered content"));
    }

    #[test]
    fn display_and_from_str_round_trip() {
        let id = ContentId::from_bytes(b"round trip test");
        let s = id.to_string();
        let parsed: ContentId = s.parse().unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn serde_json_round_trip() {
        let id = ContentId::from_bytes(b"serde test");
        let json = serde_json::to_string(&id).unwrap();
        let parsed: ContentId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn from_str_invalid_input_returns_error() {
        let result = ContentId::from_str("not-a-valid-cid");
        assert!(result.is_err());
    }

    #[test]
    fn from_str_rejects_cid_v0() {
        let result = ContentId::from_str("QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG");
        assert!(result.is_err());
    }

    #[test]
    fn from_str_rejects_non_raw_codec() {
        const DAG_PB: u64 = 0x70;
        let cid = Cid::new_v1(DAG_PB, Code::Blake3_256.digest(b"data"));
        assert!(ContentId::from_str(&cid.to_string()).is_err());
    }

    #[test]
    fn from_str_rejects_non_blake3_multihash() {
        const SHA2_256: u64 = 0x12;
        let mh = multihash::Multihash::<64>::wrap(SHA2_256, &[0u8; 32]).unwrap();
        let cid = Cid::new_v1(RAW_CODEC, mh);
        assert!(ContentId::from_str(&cid.to_string()).is_err());
    }
}
