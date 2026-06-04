use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::IdentityId;

pub const IDENTITY_ENCRYPTION_KEYS_PATH: &str = "/.well-known/jolt/encryption-keys";

const DOMAIN_SEPARATOR: &[u8] = b"jolt:identity-encryption-key-record:v1\0";
const RECORD_TYPE: &str = "jolt.identity_encryption_keys";
const RECORD_VERSION: u16 = 1;
const SUPPORTED_SUITE_FAMILY: &str = "x25519-hkdf-sha256";
const SUPPORTED_KEY_TYPE: &str = "OKP";
const SUPPORTED_CURVE: &str = "X25519";
const ACTIVE_STATUS: &str = "active";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityEncryptionKeyRecordError {
    #[error("owner public key must be 32 bytes")]
    InvalidOwnerPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("record identity does not match owner public key")]
    IdentityMismatch,

    #[error("record type is not supported")]
    UnsupportedRecordType,

    #[error("record version is not supported")]
    UnsupportedRecordVersion,

    #[error("encryption key public key must be 32 bytes")]
    InvalidEncryptionPublicKey,

    #[error("encryption key validity range is invalid")]
    InvalidKeyValidityRange,

    #[error("record has no usable encryption keys")]
    NoUsableKeys,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEncryptionKey {
    pub key_id: String,
    pub suite_family: String,
    pub key_type: String,
    pub curve: String,
    pub public_key: Vec<u8>,
    pub created_at: u64,
    pub not_before: u64,
    pub expires_at: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEncryptionKeyRecordBody {
    pub record_type: String,
    pub version: u16,
    pub owner_public_key: Vec<u8>,
    pub identity: IdentityId,
    pub keys: Vec<IdentityEncryptionKey>,
    pub sequence: u64,
    pub issued_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEncryptionKeyRecord {
    pub body: IdentityEncryptionKeyRecordBody,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedIdentityEncryptionKeys {
    pub identity: IdentityId,
    pub latest_sequence: u64,
    pub keys: Vec<IdentityEncryptionKey>,
}

impl IdentityEncryptionKeyRecord {
    pub fn new<F>(
        owner_public_key: impl Into<Vec<u8>>,
        identity: IdentityId,
        keys: Vec<IdentityEncryptionKey>,
        sequence: u64,
        issued_at: u64,
        signer: F,
    ) -> Result<Self, IdentityEncryptionKeyRecordError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = IdentityEncryptionKeyRecordBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            owner_public_key: owner_public_key.into(),
            identity,
            keys,
            sequence,
            issued_at,
        };
        validate_body(&body)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl IdentityEncryptionKeyRecordBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.record_type);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        put_bytes(&mut bytes, &self.owner_public_key);
        put_string(&mut bytes, &self.identity.to_string());
        bytes.extend_from_slice(&(self.keys.len() as u64).to_be_bytes());
        for key in &self.keys {
            put_string(&mut bytes, &key.key_id);
            put_string(&mut bytes, &key.suite_family);
            put_string(&mut bytes, &key.key_type);
            put_string(&mut bytes, &key.curve);
            put_bytes(&mut bytes, &key.public_key);
            bytes.extend_from_slice(&key.created_at.to_be_bytes());
            bytes.extend_from_slice(&key.not_before.to_be_bytes());
            match key.expires_at {
                Some(expires_at) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&expires_at.to_be_bytes());
                }
                None => bytes.push(0),
            }
            put_string(&mut bytes, &key.status);
        }
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        bytes.extend_from_slice(&self.issued_at.to_be_bytes());
        bytes
    }
}

pub fn verify_identity_encryption_key_record_for_identity(
    identity: &IdentityId,
    record: &IdentityEncryptionKeyRecord,
    now: u64,
) -> Result<VerifiedIdentityEncryptionKeys, IdentityEncryptionKeyRecordError> {
    validate_body(&record.body)?;
    if &record.body.identity != identity {
        return Err(IdentityEncryptionKeyRecordError::IdentityMismatch);
    }
    verify_signature(
        &record.body.owner_public_key,
        &record.body.canonical_bytes(),
        &record.signature,
    )?;

    let keys: Vec<_> = record
        .body
        .keys
        .iter()
        .filter(|key| is_usable_key(key, now))
        .cloned()
        .collect();
    if keys.is_empty() {
        return Err(IdentityEncryptionKeyRecordError::NoUsableKeys);
    }

    Ok(VerifiedIdentityEncryptionKeys {
        identity: record.body.identity.clone(),
        latest_sequence: record.body.sequence,
        keys,
    })
}

fn validate_body(
    body: &IdentityEncryptionKeyRecordBody,
) -> Result<(), IdentityEncryptionKeyRecordError> {
    if body.record_type != RECORD_TYPE {
        return Err(IdentityEncryptionKeyRecordError::UnsupportedRecordType);
    }
    if body.version != RECORD_VERSION {
        return Err(IdentityEncryptionKeyRecordError::UnsupportedRecordVersion);
    }
    validate_owner_public_key(&body.owner_public_key)?;
    let owner_identity = IdentityId::from_public_key(
        body.owner_public_key
            .as_slice()
            .try_into()
            .map_err(|_| IdentityEncryptionKeyRecordError::InvalidOwnerPublicKey)?,
    );
    if owner_identity != body.identity {
        return Err(IdentityEncryptionKeyRecordError::IdentityMismatch);
    }
    for key in &body.keys {
        validate_key_shape(key)?;
    }
    Ok(())
}

fn validate_key_shape(key: &IdentityEncryptionKey) -> Result<(), IdentityEncryptionKeyRecordError> {
    if key.public_key.len() != PUBLIC_KEY_LEN {
        return Err(IdentityEncryptionKeyRecordError::InvalidEncryptionPublicKey);
    }
    if let Some(expires_at) = key.expires_at {
        if expires_at <= key.not_before {
            return Err(IdentityEncryptionKeyRecordError::InvalidKeyValidityRange);
        }
    }
    Ok(())
}

fn is_usable_key(key: &IdentityEncryptionKey, now: u64) -> bool {
    key.suite_family == SUPPORTED_SUITE_FAMILY
        && key.key_type == SUPPORTED_KEY_TYPE
        && key.curve == SUPPORTED_CURVE
        && key.public_key.len() == PUBLIC_KEY_LEN
        && key.status == ACTIVE_STATUS
        && key.not_before <= now
        && key
            .expires_at
            .map(|expires_at| expires_at > now)
            .unwrap_or(true)
}

fn verify_signature(
    owner_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), IdentityEncryptionKeyRecordError> {
    validate_owner_public_key(owner_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = owner_public_key
        .try_into()
        .map_err(|_| IdentityEncryptionKeyRecordError::InvalidOwnerPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| IdentityEncryptionKeyRecordError::InvalidSignatureLength)?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| IdentityEncryptionKeyRecordError::InvalidOwnerPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);
    key.verify_strict(payload, &signature)
        .map_err(|_| IdentityEncryptionKeyRecordError::InvalidSignature)
}

fn validate_owner_public_key(public_key: &[u8]) -> Result<(), IdentityEncryptionKeyRecordError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(IdentityEncryptionKeyRecordError::InvalidOwnerPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), IdentityEncryptionKeyRecordError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(IdentityEncryptionKeyRecordError::InvalidSignatureLength)
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
    use ed25519_dalek::{Signer, SigningKey};

    use crate::{
        verify_identity_encryption_key_record_for_identity, IdentityEncryptionKey,
        IdentityEncryptionKeyRecord, IdentityEncryptionKeyRecordError, IdentityId,
    };

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn active_key() -> IdentityEncryptionKey {
        IdentityEncryptionKey {
            key_id: "enc_x25519_2026_06".to_string(),
            suite_family: "x25519-hkdf-sha256".to_string(),
            key_type: "OKP".to_string(),
            curve: "X25519".to_string(),
            public_key: vec![9; 32],
            created_at: 100,
            not_before: 100,
            expires_at: Some(200),
            status: "active".to_string(),
        }
    }

    fn record_for(
        owner: &SigningKey,
        keys: Vec<IdentityEncryptionKey>,
    ) -> IdentityEncryptionKeyRecord {
        let public_key = owner.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        IdentityEncryptionKeyRecord::new(public_key, identity, keys, 3, 100, |bytes| {
            owner.sign(bytes).to_bytes().to_vec()
        })
        .unwrap()
    }

    #[test]
    fn verifies_owner_signed_identity_encryption_key_record() {
        let owner = key(7);
        let public_key = owner.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);

        let record = IdentityEncryptionKeyRecord::new(
            public_key,
            identity.clone(),
            vec![active_key()],
            3,
            100,
            |bytes| owner.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        let verified =
            verify_identity_encryption_key_record_for_identity(&identity, &record, 150).unwrap();

        assert_eq!(verified.identity, identity);
        assert_eq!(verified.latest_sequence, 3);
        assert_eq!(verified.keys.len(), 1);
        assert_eq!(verified.keys[0].key_id, "enc_x25519_2026_06");
    }

    #[test]
    fn rejects_records_for_a_different_identity() {
        let owner = key(7);
        let other = key(8);
        let record = record_for(&owner, vec![active_key()]);
        let other_identity = IdentityId::from_public_key(other.verifying_key().to_bytes());

        assert_eq!(
            verify_identity_encryption_key_record_for_identity(&other_identity, &record, 150),
            Err(IdentityEncryptionKeyRecordError::IdentityMismatch)
        );
    }

    #[test]
    fn rejects_tampered_records() {
        let owner = key(7);
        let identity = IdentityId::from_public_key(owner.verifying_key().to_bytes());
        let mut record = record_for(&owner, vec![active_key()]);
        record.body.keys[0].key_id = "tampered".to_string();

        assert_eq!(
            verify_identity_encryption_key_record_for_identity(&identity, &record, 150),
            Err(IdentityEncryptionKeyRecordError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_records_without_current_active_supported_keys() {
        let owner = key(7);
        let identity = IdentityId::from_public_key(owner.verifying_key().to_bytes());
        let mut expired = active_key();
        expired.expires_at = Some(150);
        let mut unsupported = active_key();
        unsupported.curve = "P-256".to_string();
        let record = record_for(&owner, vec![expired, unsupported]);

        assert_eq!(
            verify_identity_encryption_key_record_for_identity(&identity, &record, 150),
            Err(IdentityEncryptionKeyRecordError::NoUsableKeys)
        );
    }
}
