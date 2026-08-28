use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use jolt_core::{
    verify_identity_authority_chain, AuthorizedDevice, AuthorizedDeviceStatus,
    DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceEncryptionPublicKey,
    EncryptedObjectRecipient, IdentityAuthorityError, IdentityEncryptionKey, IdentityId,
};
use jolt_identity::verify_signature as verify_ed25519_signature;
use jolt_network::{DaemonHandle, NetworkError};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SUPPORTED_DEVICE_ENCRYPTION_SUITE_FAMILY: &str = "x25519-hkdf-sha256";

#[derive(Clone, Default)]
pub struct DeviceAuthorityStore;

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizeDeviceRequest {
    pub signing_public_key: Option<Vec<u8>>,
    #[serde(default)]
    pub encryption_keys: Vec<DeviceEncryptionPublicKey>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RevokeDeviceRequest {
    pub accepted_through_device_sequence: Option<u64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceAuthorityResponse {
    pub identity: String,
    pub latest_sequence: u64,
    pub devices: Vec<DeviceAuthorityDeviceView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceAuthorityMutationResponse {
    pub identity: String,
    pub latest_sequence: u64,
    pub device: DeviceAuthorityDeviceView,
    pub devices: Vec<DeviceAuthorityDeviceView>,
    pub authority_records: Vec<DeviceAuthorizationRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceAuthorityDeviceView {
    pub device_id: String,
    pub signing_public_key: Vec<u8>,
    pub encryption_keys: Vec<DeviceEncryptionPublicKey>,
    pub capabilities: Vec<String>,
    pub label: Option<String>,
    pub status: String,
    pub authorized_at: u64,
    pub revoked_at: Option<u64>,
    pub revocation_reason: Option<String>,
    pub accepted_through_device_sequence: Option<u64>,
}

#[derive(Debug, Error)]
pub enum DeviceAuthorityError {
    #[error("daemon has no local identity")]
    MissingLocalIdentity,
    #[error("invalid local identity: {0}")]
    InvalidLocalIdentity(String),
    #[error("invalid device enrollment: {0}")]
    InvalidEnrollment(String),
    #[error("unknown device: {0}")]
    UnknownDevice(String),
    #[error("device authority verification failed: {0}")]
    Authority(#[from] IdentityAuthorityError),
    #[error("daemon signing failed: {0}")]
    Network(#[from] NetworkError),
}

impl DeviceAuthorityStore {
    pub fn new() -> Self {
        Self
    }

    pub async fn list(
        &self,
        daemon: &DaemonHandle,
    ) -> Result<DeviceAuthorityResponse, DeviceAuthorityError> {
        let records = daemon.local_device_authority().await?;
        response_from_records(&records)
    }

    pub async fn authorize_device(
        &self,
        daemon: &DaemonHandle,
        request: AuthorizeDeviceRequest,
    ) -> Result<DeviceAuthorityMutationResponse, DeviceAuthorityError> {
        let existing_records = daemon.local_device_authority().await?;
        let identity = existing_records
            .first()
            .map(|record| record.body.identity.clone())
            .ok_or(DeviceAuthorityError::MissingLocalIdentity)?;
        let created_at = unix_now();
        let signing_public_key = request.signing_public_key.ok_or_else(|| {
            DeviceAuthorityError::InvalidEnrollment(
                "joining installation must supply its signing public key".to_string(),
            )
        })?;
        let public_key: [u8; 32] = signing_public_key.as_slice().try_into().map_err(|_| {
            DeviceAuthorityError::InvalidEnrollment(
                "signing public key must contain 32 bytes".to_string(),
            )
        })?;
        verify_ed25519_signature(&signing_public_key, b"", &[0; 64]).map_err(|error| {
            DeviceAuthorityError::InvalidEnrollment(format!(
                "signing public key is not a valid Ed25519 key: {error}"
            ))
        })?;
        if request.encryption_keys.is_empty() {
            return Err(DeviceAuthorityError::InvalidEnrollment(
                "joining installation must supply an encryption public key".to_string(),
            ));
        }
        if request
            .encryption_keys
            .iter()
            .any(|key| key.suite_family != SUPPORTED_DEVICE_ENCRYPTION_SUITE_FAMILY)
        {
            return Err(DeviceAuthorityError::InvalidEnrollment(format!(
                "encryption keys must use {SUPPORTED_DEVICE_ENCRYPTION_SUITE_FAMILY}"
            )));
        }
        if request
            .encryption_keys
            .iter()
            .any(|key| key.public_key.len() != 32)
        {
            return Err(DeviceAuthorityError::InvalidEnrollment(
                "encryption public keys must contain 32 bytes".to_string(),
            ));
        }
        let device_id = format!("dev_{}", IdentityId::from_public_key(public_key));
        let authority = verify_identity_authority_chain(&identity, &existing_records)?;
        if authority.devices.contains_key(&device_id) {
            return Err(DeviceAuthorityError::InvalidEnrollment(format!(
                "device {device_id} is already present in the authority chain"
            )));
        }
        let mut encryption_key_ids: HashSet<&str> = authority
            .devices
            .values()
            .flat_map(|device| device.encryption_keys.iter())
            .map(|key| key.key_id.as_str())
            .collect();
        for key in &request.encryption_keys {
            if !encryption_key_ids.insert(&key.key_id) {
                return Err(DeviceAuthorityError::InvalidEnrollment(format!(
                    "encryption key id {} is already present in the authority chain",
                    key.key_id
                )));
            }
        }
        let operation = DeviceAuthorizationOperation::authorize_device_with_encryption_keys(
            device_id.clone(),
            signing_public_key,
            request.encryption_keys,
            default_device_capabilities(),
            request.label,
            created_at,
        );
        let records = daemon.append_local_device_authority(operation).await?;

        let response = response_from_records(&records)?;
        let authorized_device = response
            .devices
            .iter()
            .find(|device| device.device_id == device_id)
            .cloned()
            .ok_or_else(|| DeviceAuthorityError::UnknownDevice(device_id.clone()))?;
        Ok(DeviceAuthorityMutationResponse {
            identity: response.identity,
            latest_sequence: response.latest_sequence,
            device: authorized_device,
            devices: response.devices,
            authority_records: records,
        })
    }

    pub async fn active_authorized_device_encryption_recipients(
        &self,
        daemon: &DaemonHandle,
    ) -> Result<Vec<EncryptedObjectRecipient>, DeviceAuthorityError> {
        let records = daemon.local_device_authority().await?;
        let identity = records
            .first()
            .map(|record| record.body.identity.clone())
            .ok_or(DeviceAuthorityError::MissingLocalIdentity)?;
        let verified = verify_identity_authority_chain(&identity, &records)?;
        Ok(verified
            .devices
            .values()
            .filter(|device| device.status == AuthorizedDeviceStatus::Active)
            .flat_map(|device| {
                device
                    .encryption_keys
                    .iter()
                    .map(|key| EncryptedObjectRecipient {
                        identity: identity.clone(),
                        key: identity_encryption_key(key),
                    })
            })
            .collect())
    }

    pub async fn revoke_device(
        &self,
        daemon: &DaemonHandle,
        device_id: String,
        request: RevokeDeviceRequest,
    ) -> Result<DeviceAuthorityMutationResponse, DeviceAuthorityError> {
        let operation = DeviceAuthorizationOperation::revoke_device(
            device_id.clone(),
            request.accepted_through_device_sequence,
            request.reason,
            unix_now(),
        );
        let records = daemon.append_local_device_authority(operation).await?;
        let response = response_from_records(&records)?;
        let device = response
            .devices
            .iter()
            .find(|device| device.device_id == device_id)
            .cloned()
            .ok_or_else(|| DeviceAuthorityError::UnknownDevice(device_id.clone()))?;
        Ok(DeviceAuthorityMutationResponse {
            identity: response.identity,
            latest_sequence: response.latest_sequence,
            device,
            devices: response.devices,
            authority_records: records,
        })
    }
}

fn response_from_records(
    records: &[DeviceAuthorizationRecord],
) -> Result<DeviceAuthorityResponse, DeviceAuthorityError> {
    let identity = records
        .first()
        .map(|record| record.body.identity.clone())
        .ok_or(DeviceAuthorityError::MissingLocalIdentity)?;
    let verified = verify_identity_authority_chain(&identity, records)?;
    Ok(DeviceAuthorityResponse {
        identity: format!("{}.jolt", verified.identity),
        latest_sequence: verified.latest_sequence,
        devices: verified.devices.values().map(device_view).collect(),
    })
}

fn device_view(device: &AuthorizedDevice) -> DeviceAuthorityDeviceView {
    DeviceAuthorityDeviceView {
        device_id: device.device_id.clone(),
        signing_public_key: device.signing_public_key.clone(),
        encryption_keys: device.encryption_keys.clone(),
        capabilities: device.capabilities.clone(),
        label: device.label.clone(),
        status: match device.status {
            AuthorizedDeviceStatus::Active => "active",
            AuthorizedDeviceStatus::Revoked => "revoked",
        }
        .to_string(),
        authorized_at: device.authorized_at,
        revoked_at: device.revoked_at,
        revocation_reason: device.revocation_reason.clone(),
        accepted_through_device_sequence: device.accepted_through_device_sequence,
    }
}

fn identity_encryption_key(key: &DeviceEncryptionPublicKey) -> IdentityEncryptionKey {
    IdentityEncryptionKey {
        key_id: key.key_id.clone(),
        suite_family: key.suite_family.clone(),
        key_type: "OKP".to_string(),
        curve: "X25519".to_string(),
        public_key: key.public_key.clone(),
        created_at: key.created_at,
        not_before: key.created_at,
        expires_at: None,
        status: "active".to_string(),
    }
}

fn default_device_capabilities() -> Vec<String> {
    vec![
        "identity:write".to_string(),
        "app:grant".to_string(),
        "encrypt:receive".to_string(),
    ]
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
