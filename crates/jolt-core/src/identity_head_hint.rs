use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{update_log::UpdateLogEntryHash, IdentityId};

const DOMAIN_SEPARATOR: &[u8] = b"jolt:identity-head-hint:v1\0";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityHeadHintError {
    #[error("owner public key must be 32 bytes")]
    InvalidOwnerPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("hint identity does not match owner public key")]
    IdentityMismatch,

    #[error("hint has expired")]
    Expired,

    #[error("hint expiry must be after observation time")]
    InvalidExpiry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityHeadHintBody {
    pub owner_public_key: Vec<u8>,
    pub identity: IdentityId,
    pub provider_peer_id: String,
    pub provider_addrs: Vec<String>,
    pub relay_hint: Option<String>,
    pub latest_sequence: u64,
    pub update_log_head: UpdateLogEntryHash,
    pub observed_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityHeadHint {
    pub body: IdentityHeadHintBody,
    pub signature: Vec<u8>,
}

impl IdentityHeadHint {
    #[allow(clippy::too_many_arguments)]
    pub fn new<F>(
        owner_public_key: impl Into<Vec<u8>>,
        identity: IdentityId,
        provider_peer_id: String,
        provider_addrs: Vec<String>,
        relay_hint: Option<String>,
        latest_sequence: u64,
        update_log_head: UpdateLogEntryHash,
        observed_at: u64,
        expires_at: u64,
        signer: F,
    ) -> Result<Self, IdentityHeadHintError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = IdentityHeadHintBody {
            owner_public_key: owner_public_key.into(),
            identity,
            provider_peer_id,
            provider_addrs,
            relay_hint,
            latest_sequence,
            update_log_head,
            observed_at,
            expires_at,
        };
        validate_body(&body)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }

    pub fn verify_at(&self, now: u64) -> Result<(), IdentityHeadHintError> {
        validate_body(&self.body)?;
        if self.body.expires_at <= now {
            return Err(IdentityHeadHintError::Expired);
        }
        verify_signature(
            &self.body.owner_public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }
}

impl IdentityHeadHintBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_bytes(&mut bytes, &self.owner_public_key);
        put_string(&mut bytes, &self.identity.to_string());
        put_string(&mut bytes, &self.provider_peer_id);
        bytes.extend_from_slice(&(self.provider_addrs.len() as u64).to_be_bytes());
        for addr in &self.provider_addrs {
            put_string(&mut bytes, addr);
        }
        match &self.relay_hint {
            Some(relay_hint) => {
                bytes.push(1);
                put_string(&mut bytes, relay_hint);
            }
            None => bytes.push(0),
        }
        bytes.extend_from_slice(&self.latest_sequence.to_be_bytes());
        bytes.extend_from_slice(&self.update_log_head.0);
        bytes.extend_from_slice(&self.observed_at.to_be_bytes());
        bytes.extend_from_slice(&self.expires_at.to_be_bytes());
        bytes
    }
}

fn validate_body(body: &IdentityHeadHintBody) -> Result<(), IdentityHeadHintError> {
    validate_owner_public_key(&body.owner_public_key)?;
    let owner_identity = IdentityId::from_public_key(
        body.owner_public_key
            .as_slice()
            .try_into()
            .map_err(|_| IdentityHeadHintError::InvalidOwnerPublicKey)?,
    );
    if owner_identity != body.identity {
        return Err(IdentityHeadHintError::IdentityMismatch);
    }
    if body.expires_at <= body.observed_at {
        return Err(IdentityHeadHintError::InvalidExpiry);
    }
    Ok(())
}

fn verify_signature(
    owner_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), IdentityHeadHintError> {
    validate_owner_public_key(owner_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = owner_public_key
        .try_into()
        .map_err(|_| IdentityHeadHintError::InvalidOwnerPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| IdentityHeadHintError::InvalidSignatureLength)?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| IdentityHeadHintError::InvalidOwnerPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);
    key.verify_strict(payload, &signature)
        .map_err(|_| IdentityHeadHintError::InvalidSignature)
}

fn validate_owner_public_key(public_key: &[u8]) -> Result<(), IdentityHeadHintError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(IdentityHeadHintError::InvalidOwnerPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), IdentityHeadHintError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(IdentityHeadHintError::InvalidSignatureLength)
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
    use crate::UpdateLogEntryHash;
    use ed25519_dalek::{Signer, SigningKey};

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn verifies_owner_signed_hint() {
        let key = key(7);
        let public_key = key.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        let hint = IdentityHeadHint::new(
            public_key,
            identity,
            "provider".to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            None,
            4,
            UpdateLogEntryHash([9; 32]),
            100,
            200,
            |bytes| key.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        assert_eq!(hint.verify_at(150), Ok(()));
    }

    #[test]
    fn rejects_tampered_hint() {
        let key = key(7);
        let public_key = key.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        let mut hint = IdentityHeadHint::new(
            public_key,
            identity,
            "provider".to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            None,
            4,
            UpdateLogEntryHash([9; 32]),
            100,
            200,
            |bytes| key.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();
        hint.body.latest_sequence = 5;

        assert_eq!(
            hint.verify_at(150),
            Err(IdentityHeadHintError::InvalidSignature)
        );
    }
}
