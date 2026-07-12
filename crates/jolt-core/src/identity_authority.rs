use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::IdentityId;

const DOMAIN_SEPARATOR: &[u8] = b"jolt:identity-authority-record:v1\0";
pub const IDENTITY_AUTHORITY_PATH: &str = "/.well-known/jolt/device-authority";
const RECORD_TYPE: &str = "jolt.identity_authority_record";
const RECORD_VERSION: u16 = 1;
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentityAuthorityError {
    #[error("authority chain is empty")]
    EmptyChain,

    #[error("root public key must be 32 bytes")]
    InvalidRootPublicKey,

    #[error("device signing public key must be 32 bytes")]
    InvalidDeviceSigningPublicKey,

    #[error("device encryption public key must be 32 bytes")]
    InvalidDeviceEncryptionPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("record identity does not match root public key")]
    IdentityMismatch,

    #[error("record type is not supported")]
    UnsupportedRecordType,

    #[error("record version is not supported")]
    UnsupportedRecordVersion,

    #[error("genesis authority record must have sequence 0")]
    InvalidGenesisSequence,

    #[error("genesis authority record must not have a previous record hash")]
    GenesisHasPreviousHash,

    #[error("authority record at index {index} has sequence {actual}, expected {expected}")]
    OutOfOrderSequence {
        index: usize,
        expected: u64,
        actual: u64,
    },

    #[error("authority record at index {index} changes root public key")]
    RootChanged { index: usize },

    #[error("authority record at index {index} has a broken previous-record hash")]
    BrokenPreviousHash { index: usize },

    #[error("device id is required")]
    EmptyDeviceId,

    #[error("cannot revoke unknown device {0}")]
    UnknownDeviceRevocation(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceAuthorizationRecordHash(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEncryptionPublicKey {
    pub key_id: String,
    pub suite_family: String,
    pub public_key: Vec<u8>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceAuthorizationOperation {
    AuthorizeDevice {
        device_id: String,
        device_signing_public_key: Vec<u8>,
        device_encryption_keys: Vec<DeviceEncryptionPublicKey>,
        capabilities: Vec<String>,
        label: Option<String>,
        created_at: u64,
    },
    RevokeDevice {
        device_id: String,
        accepted_through_device_sequence: Option<u64>,
        reason: Option<String>,
        created_at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorizationRecordBody {
    pub record_type: String,
    pub version: u16,
    pub root_public_key: Vec<u8>,
    pub identity: IdentityId,
    pub sequence: u64,
    pub previous_record_hash: Option<DeviceAuthorizationRecordHash>,
    pub operation: DeviceAuthorizationOperation,
    pub issued_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorizationRecord {
    pub body: DeviceAuthorizationRecordBody,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedIdentityAuthority {
    pub identity: IdentityId,
    pub latest_sequence: u64,
    pub devices: BTreeMap<String, AuthorizedDevice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedDevice {
    pub device_id: String,
    pub signing_public_key: Vec<u8>,
    pub encryption_keys: Vec<DeviceEncryptionPublicKey>,
    pub capabilities: Vec<String>,
    pub label: Option<String>,
    pub status: AuthorizedDeviceStatus,
    pub authorized_at: u64,
    pub revoked_at: Option<u64>,
    pub revocation_reason: Option<String>,
    pub accepted_through_device_sequence: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizedDeviceStatus {
    Active,
    Revoked,
}

impl DeviceAuthorizationOperation {
    pub fn authorize_device(
        device_id: impl Into<String>,
        device_signing_public_key: impl Into<Vec<u8>>,
        capabilities: Vec<String>,
        label: Option<String>,
        created_at: u64,
    ) -> Self {
        Self::AuthorizeDevice {
            device_id: device_id.into(),
            device_signing_public_key: device_signing_public_key.into(),
            device_encryption_keys: Vec::new(),
            capabilities,
            label,
            created_at,
        }
    }

    pub fn authorize_device_with_encryption_keys(
        device_id: impl Into<String>,
        device_signing_public_key: impl Into<Vec<u8>>,
        device_encryption_keys: Vec<DeviceEncryptionPublicKey>,
        capabilities: Vec<String>,
        label: Option<String>,
        created_at: u64,
    ) -> Self {
        Self::AuthorizeDevice {
            device_id: device_id.into(),
            device_signing_public_key: device_signing_public_key.into(),
            device_encryption_keys,
            capabilities,
            label,
            created_at,
        }
    }

    pub fn revoke_device(
        device_id: impl Into<String>,
        accepted_through_device_sequence: Option<u64>,
        reason: Option<String>,
        created_at: u64,
    ) -> Self {
        Self::RevokeDevice {
            device_id: device_id.into(),
            accepted_through_device_sequence,
            reason,
            created_at,
        }
    }
}

impl DeviceAuthorizationRecord {
    pub fn genesis<F>(
        root_public_key: impl Into<Vec<u8>>,
        identity: IdentityId,
        operation: DeviceAuthorizationOperation,
        issued_at: u64,
        signer: F,
    ) -> Result<Self, IdentityAuthorityError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = DeviceAuthorizationRecordBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            root_public_key: root_public_key.into(),
            identity,
            sequence: 0,
            previous_record_hash: None,
            operation,
            issued_at,
        };
        Self::sign_body(body, signer)
    }

    pub fn append<F>(
        &self,
        operation: DeviceAuthorizationOperation,
        issued_at: u64,
        signer: F,
    ) -> Result<Self, IdentityAuthorityError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = DeviceAuthorizationRecordBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            root_public_key: self.body.root_public_key.clone(),
            identity: self.body.identity.clone(),
            sequence: self.body.sequence + 1,
            previous_record_hash: Some(self.record_hash()),
            operation,
            issued_at,
        };
        Self::sign_body(body, signer)
    }

    pub fn record_hash(&self) -> DeviceAuthorizationRecordHash {
        DeviceAuthorizationRecordHash(*blake3::hash(&self.body.canonical_bytes()).as_bytes())
    }

    pub fn verify_signature(&self) -> Result<(), IdentityAuthorityError> {
        verify_signature(
            &self.body.root_public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }

    fn sign_body<F>(
        body: DeviceAuthorizationRecordBody,
        signer: F,
    ) -> Result<Self, IdentityAuthorityError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        validate_body(&body)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl DeviceAuthorizationRecordBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.record_type);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        put_bytes(&mut bytes, &self.root_public_key);
        put_string(&mut bytes, &self.identity.to_string());
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        match &self.previous_record_hash {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&hash.0);
            }
            None => bytes.push(0),
        }
        self.operation.encode(&mut bytes);
        bytes.extend_from_slice(&self.issued_at.to_be_bytes());
        bytes
    }
}

impl DeviceAuthorizationOperation {
    fn encode(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::AuthorizeDevice {
                device_id,
                device_signing_public_key,
                device_encryption_keys,
                capabilities,
                label,
                created_at,
            } => {
                bytes.push(0);
                put_string(bytes, device_id);
                put_bytes(bytes, device_signing_public_key);
                bytes.extend_from_slice(&(device_encryption_keys.len() as u64).to_be_bytes());
                for key in device_encryption_keys {
                    key.encode(bytes);
                }
                bytes.extend_from_slice(&(capabilities.len() as u64).to_be_bytes());
                for capability in capabilities {
                    put_string(bytes, capability);
                }
                put_optional_string(bytes, label.as_deref());
                bytes.extend_from_slice(&created_at.to_be_bytes());
            }
            Self::RevokeDevice {
                device_id,
                accepted_through_device_sequence,
                reason,
                created_at,
            } => {
                bytes.push(1);
                put_string(bytes, device_id);
                match accepted_through_device_sequence {
                    Some(sequence) => {
                        bytes.push(1);
                        bytes.extend_from_slice(&sequence.to_be_bytes());
                    }
                    None => bytes.push(0),
                }
                put_optional_string(bytes, reason.as_deref());
                bytes.extend_from_slice(&created_at.to_be_bytes());
            }
        }
    }
}

impl DeviceEncryptionPublicKey {
    fn encode(&self, bytes: &mut Vec<u8>) {
        put_string(bytes, &self.key_id);
        put_string(bytes, &self.suite_family);
        put_bytes(bytes, &self.public_key);
        bytes.extend_from_slice(&self.created_at.to_be_bytes());
    }
}

impl VerifiedIdentityAuthority {
    pub fn device_can_write(&self, device_id: &str, device_sequence: u64) -> bool {
        let Some(device) = self.devices.get(device_id) else {
            return false;
        };
        match device.status {
            AuthorizedDeviceStatus::Active => true,
            AuthorizedDeviceStatus::Revoked => device
                .accepted_through_device_sequence
                .map(|accepted| device_sequence <= accepted)
                .unwrap_or(false),
        }
    }
}

pub fn verify_identity_authority_chain(
    identity: &IdentityId,
    records: &[DeviceAuthorizationRecord],
) -> Result<VerifiedIdentityAuthority, IdentityAuthorityError> {
    let Some(first) = records.first() else {
        return Err(IdentityAuthorityError::EmptyChain);
    };

    let mut devices = BTreeMap::new();
    let mut previous_hash = None;
    let root_public_key = first.body.root_public_key.clone();

    for (index, record) in records.iter().enumerate() {
        validate_body(&record.body)?;
        if &record.body.identity != identity {
            return Err(IdentityAuthorityError::IdentityMismatch);
        }
        if record.body.root_public_key != root_public_key {
            return Err(IdentityAuthorityError::RootChanged { index });
        }
        record.verify_signature()?;

        if index == 0 {
            if record.body.sequence != 0 {
                return Err(IdentityAuthorityError::InvalidGenesisSequence);
            }
            if record.body.previous_record_hash.is_some() {
                return Err(IdentityAuthorityError::GenesisHasPreviousHash);
            }
        } else {
            let expected = index as u64;
            if record.body.sequence != expected {
                return Err(IdentityAuthorityError::OutOfOrderSequence {
                    index,
                    expected,
                    actual: record.body.sequence,
                });
            }
            if record.body.previous_record_hash != previous_hash {
                return Err(IdentityAuthorityError::BrokenPreviousHash { index });
            }
        }

        apply_operation(&mut devices, &record.body.operation)?;
        previous_hash = Some(record.record_hash());
    }

    Ok(VerifiedIdentityAuthority {
        identity: identity.clone(),
        latest_sequence: records.last().expect("non-empty records").body.sequence,
        devices,
    })
}

fn apply_operation(
    devices: &mut BTreeMap<String, AuthorizedDevice>,
    operation: &DeviceAuthorizationOperation,
) -> Result<(), IdentityAuthorityError> {
    match operation {
        DeviceAuthorizationOperation::AuthorizeDevice {
            device_id,
            device_signing_public_key,
            device_encryption_keys,
            capabilities,
            label,
            created_at,
        } => {
            validate_device_id(device_id)?;
            validate_device_signing_public_key(device_signing_public_key)?;
            for key in device_encryption_keys {
                validate_device_encryption_key(key)?;
            }
            devices.insert(
                device_id.clone(),
                AuthorizedDevice {
                    device_id: device_id.clone(),
                    signing_public_key: device_signing_public_key.clone(),
                    encryption_keys: device_encryption_keys.clone(),
                    capabilities: capabilities.clone(),
                    label: label.clone(),
                    status: AuthorizedDeviceStatus::Active,
                    authorized_at: *created_at,
                    revoked_at: None,
                    revocation_reason: None,
                    accepted_through_device_sequence: None,
                },
            );
        }
        DeviceAuthorizationOperation::RevokeDevice {
            device_id,
            accepted_through_device_sequence,
            reason,
            created_at,
        } => {
            validate_device_id(device_id)?;
            let Some(device) = devices.get_mut(device_id) else {
                return Err(IdentityAuthorityError::UnknownDeviceRevocation(
                    device_id.clone(),
                ));
            };
            device.status = AuthorizedDeviceStatus::Revoked;
            device.revoked_at = Some(*created_at);
            device.revocation_reason = reason.clone();
            device.accepted_through_device_sequence = *accepted_through_device_sequence;
        }
    }
    Ok(())
}

fn validate_body(body: &DeviceAuthorizationRecordBody) -> Result<(), IdentityAuthorityError> {
    if body.record_type != RECORD_TYPE {
        return Err(IdentityAuthorityError::UnsupportedRecordType);
    }
    if body.version != RECORD_VERSION {
        return Err(IdentityAuthorityError::UnsupportedRecordVersion);
    }
    validate_root_public_key(&body.root_public_key)?;
    let owner_identity = IdentityId::from_public_key(
        body.root_public_key
            .as_slice()
            .try_into()
            .map_err(|_| IdentityAuthorityError::InvalidRootPublicKey)?,
    );
    if owner_identity != body.identity {
        return Err(IdentityAuthorityError::IdentityMismatch);
    }
    validate_operation(&body.operation)
}

fn validate_operation(
    operation: &DeviceAuthorizationOperation,
) -> Result<(), IdentityAuthorityError> {
    match operation {
        DeviceAuthorizationOperation::AuthorizeDevice {
            device_id,
            device_signing_public_key,
            device_encryption_keys,
            ..
        } => {
            validate_device_id(device_id)?;
            validate_device_signing_public_key(device_signing_public_key)?;
            for key in device_encryption_keys {
                validate_device_encryption_key(key)?;
            }
            Ok(())
        }
        DeviceAuthorizationOperation::RevokeDevice { device_id, .. } => {
            validate_device_id(device_id)
        }
    }
}

fn validate_device_id(device_id: &str) -> Result<(), IdentityAuthorityError> {
    if device_id.trim().is_empty() {
        Err(IdentityAuthorityError::EmptyDeviceId)
    } else {
        Ok(())
    }
}

fn validate_root_public_key(public_key: &[u8]) -> Result<(), IdentityAuthorityError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(IdentityAuthorityError::InvalidRootPublicKey)
    }
}

fn validate_device_signing_public_key(public_key: &[u8]) -> Result<(), IdentityAuthorityError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(IdentityAuthorityError::InvalidDeviceSigningPublicKey)
    }
}

fn validate_device_encryption_key(
    key: &DeviceEncryptionPublicKey,
) -> Result<(), IdentityAuthorityError> {
    if key.public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(IdentityAuthorityError::InvalidDeviceEncryptionPublicKey)
    }
}

fn verify_signature(
    root_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), IdentityAuthorityError> {
    validate_root_public_key(root_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = root_public_key
        .try_into()
        .map_err(|_| IdentityAuthorityError::InvalidRootPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| IdentityAuthorityError::InvalidSignatureLength)?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| IdentityAuthorityError::InvalidRootPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);
    key.verify_strict(payload, &signature)
        .map_err(|_| IdentityAuthorityError::InvalidSignature)
}

fn validate_signature_len(signature: &[u8]) -> Result<(), IdentityAuthorityError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(IdentityAuthorityError::InvalidSignatureLength)
    }
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_optional_string(out: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            out.push(1);
            put_string(out, value);
        }
        None => out.push(0),
    }
}
