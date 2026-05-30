use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::IdentityId;

const DOMAIN_SEPARATOR: &[u8] = b"jolt:relay-record:v1\0";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RelayRecordError {
    #[error("relay public key must be 32 bytes")]
    InvalidRelayPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("relay id does not match relay public key")]
    RelayIdMismatch,

    #[error("relay record has expired")]
    ExpiredRecord,

    #[error("relay record expires before it was observed")]
    InvalidTimeRange,

    #[error("unknown relay capability discriminant {0}")]
    UnknownCapability(u8),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayRecordCapability {
    Bootstrap,
    Discovery,
    Pinning,
}

impl RelayRecordCapability {
    fn encode(&self) -> u8 {
        match self {
            Self::Bootstrap => 0,
            Self::Discovery => 1,
            Self::Pinning => 2,
        }
    }
}

impl TryFrom<u8> for RelayRecordCapability {
    type Error = RelayRecordError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Bootstrap),
            1 => Ok(Self::Discovery),
            2 => Ok(Self::Pinning),
            other => Err(RelayRecordError::UnknownCapability(other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayRecordBody {
    pub relay_id: IdentityId,
    pub relay_public_key: Vec<u8>,
    pub peer_id: String,
    pub addrs: Vec<String>,
    pub capabilities: Vec<RelayRecordCapability>,
    pub observed_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayRecord {
    pub body: RelayRecordBody,
    pub signature: Vec<u8>,
}

impl RelayRecord {
    pub fn new<F>(
        relay_public_key: impl Into<Vec<u8>>,
        peer_id: String,
        addrs: Vec<String>,
        capabilities: Vec<RelayRecordCapability>,
        observed_at: u64,
        expires_at: u64,
        signer: F,
    ) -> Result<Self, RelayRecordError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let relay_public_key = relay_public_key.into();
        let relay_id = identity_from_public_key_bytes(&relay_public_key)?;
        let body = RelayRecordBody {
            relay_id,
            relay_public_key,
            peer_id,
            addrs,
            capabilities,
            observed_at,
            expires_at,
        };
        Self::sign_body(body, signer)
    }

    pub fn verify_at(&self, now: u64) -> Result<(), RelayRecordError> {
        self.verify_signature()?;
        if self.body.expires_at <= now {
            return Err(RelayRecordError::ExpiredRecord);
        }
        Ok(())
    }

    pub fn verify_signature(&self) -> Result<(), RelayRecordError> {
        validate_time_range(&self.body)?;
        let expected_id = identity_from_public_key_bytes(&self.body.relay_public_key)?;
        if self.body.relay_id != expected_id {
            return Err(RelayRecordError::RelayIdMismatch);
        }
        verify_signature(
            &self.body.relay_public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }

    fn sign_body<F>(body: RelayRecordBody, signer: F) -> Result<Self, RelayRecordError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        validate_time_range(&body)?;
        identity_from_public_key_bytes(&body.relay_public_key)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl RelayRecordBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.relay_id.to_string());
        put_bytes(&mut bytes, &self.relay_public_key);
        put_string(&mut bytes, &self.peer_id);
        bytes.extend_from_slice(&(self.addrs.len() as u64).to_be_bytes());
        for addr in &self.addrs {
            put_string(&mut bytes, addr);
        }
        bytes.extend_from_slice(&(self.capabilities.len() as u64).to_be_bytes());
        for capability in &self.capabilities {
            bytes.push(capability.encode());
        }
        bytes.extend_from_slice(&self.observed_at.to_be_bytes());
        bytes.extend_from_slice(&self.expires_at.to_be_bytes());
        bytes
    }
}

fn verify_signature(
    relay_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), RelayRecordError> {
    validate_public_key_len(relay_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = relay_public_key
        .try_into()
        .map_err(|_| RelayRecordError::InvalidRelayPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| RelayRecordError::InvalidSignatureLength)?;

    let verifying_key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| RelayRecordError::InvalidRelayPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);

    use ed25519_dalek::Verifier;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| RelayRecordError::InvalidSignature)
}

fn identity_from_public_key_bytes(public_key: &[u8]) -> Result<IdentityId, RelayRecordError> {
    validate_public_key_len(public_key)?;
    Ok(IdentityId::from_public_key(
        public_key
            .try_into()
            .map_err(|_| RelayRecordError::InvalidRelayPublicKey)?,
    ))
}

fn validate_public_key_len(public_key: &[u8]) -> Result<(), RelayRecordError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(RelayRecordError::InvalidRelayPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), RelayRecordError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(RelayRecordError::InvalidSignatureLength)
    }
}

fn validate_time_range(body: &RelayRecordBody) -> Result<(), RelayRecordError> {
    if body.expires_at <= body.observed_at {
        Err(RelayRecordError::InvalidTimeRange)
    } else {
        Ok(())
    }
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    put_bytes(bytes, value.as_bytes());
}

fn put_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
        use ed25519_dalek::Signer;
        key.sign(bytes).to_bytes().to_vec()
    }

    fn relay_record(key: &SigningKey) -> RelayRecord {
        RelayRecord::new(
            key.verifying_key().to_bytes(),
            "12D3KooWRelayPeer".to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            vec![
                RelayRecordCapability::Bootstrap,
                RelayRecordCapability::Discovery,
                RelayRecordCapability::Pinning,
            ],
            100,
            200,
            |bytes| sign(key, bytes),
        )
        .unwrap()
    }

    #[test]
    fn relay_record_verifies_with_relay_signature() {
        let key = signing_key();
        let record = relay_record(&key);

        assert_eq!(
            record.body.relay_id,
            IdentityId::from_public_key(key.verifying_key().to_bytes())
        );
        assert_eq!(record.verify_at(150), Ok(()));
    }

    #[test]
    fn relay_record_rejects_invalid_signature() {
        let key = signing_key();
        let mut record = relay_record(&key);
        record
            .body
            .addrs
            .push("/ip4/127.0.0.1/tcp/4002".to_string());

        assert_eq!(
            record.verify_at(150),
            Err(RelayRecordError::InvalidSignature)
        );
    }

    #[test]
    fn relay_record_rejects_expired_record() {
        let key = signing_key();
        let record = relay_record(&key);

        assert_eq!(record.verify_at(200), Err(RelayRecordError::ExpiredRecord));
    }

    #[test]
    fn relay_record_rejects_unknown_capability_discriminant() {
        assert!(RelayRecordCapability::try_from(3).is_err());
    }
}
