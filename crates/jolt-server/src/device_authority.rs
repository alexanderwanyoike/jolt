use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jolt_core::{
    generate_identity_encryption_keypair, verify_identity_authority_chain, AuthorizedDevice,
    AuthorizedDeviceStatus, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
    DeviceAuthorizationRecordBody, DeviceEncryptionPublicKey, EncryptedObjectRecipient,
    IdentityAuthorityError, IdentityEncryptionKey, IdentityEncryptionPrivateKey, IdentityId,
    JoltAddress, IDENTITY_AUTHORITY_PATH,
};
use jolt_identity::NodeIdentity;
use jolt_network::{DaemonHandle, NetworkError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

const LEGACY_ROOT_DEVICE_ID: &str = "dev_legacy_root";
static TEMP_AUTHORITY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone)]
pub struct DeviceAuthorityStore {
    state: Arc<Mutex<DeviceAuthorityState>>,
}

#[derive(Default)]
struct DeviceAuthorityState {
    records: Vec<DeviceAuthorizationRecord>,
    generated_devices: Vec<GeneratedDeviceRecord>,
}

struct GeneratedDeviceRecord {
    _device_id: String,
    _identity: NodeIdentity,
    _encryption_private_key: IdentityEncryptionPrivateKey,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorizeDeviceRequest {
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
    #[error("unknown device: {0}")]
    UnknownDevice(String),
    #[error("device authority verification failed: {0}")]
    Authority(#[from] IdentityAuthorityError),
    #[error("daemon signing failed: {0}")]
    Network(#[from] NetworkError),
}

impl DeviceAuthorityStore {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(DeviceAuthorityState::default())),
        }
    }

    pub async fn list(
        &self,
        daemon: &DaemonHandle,
    ) -> Result<DeviceAuthorityResponse, DeviceAuthorityError> {
        self.ensure_bootstrapped(daemon).await?;
        let state = self.state.lock().await;
        response_from_records(&state.records)
    }

    pub async fn authorize_device(
        &self,
        daemon: &DaemonHandle,
        request: AuthorizeDeviceRequest,
    ) -> Result<DeviceAuthorityMutationResponse, DeviceAuthorityError> {
        self.ensure_bootstrapped(daemon).await?;
        let (identity, _) = local_identity_parts(daemon).await?;
        let device = NodeIdentity::generate();
        let device_id = format!("dev_{}", device.identity_id());
        let created_at = unix_now();
        let (device_encryption_key, device_encryption_private_key) =
            generate_identity_encryption_keypair(
                identity,
                format!("enc_x25519_{device_id}_v0"),
                created_at,
            );
        let operation = DeviceAuthorizationOperation::authorize_device_with_encryption_keys(
            device_id.clone(),
            device.public_key_bytes(),
            vec![device_encryption_public_key(&device_encryption_key)],
            default_device_capabilities(),
            request.label,
            created_at,
        );
        let record = self.append_signed_record(daemon, operation).await?;

        let mut state = self.state.lock().await;
        let mut records = state.records.clone();
        records.push(record);
        let response = response_from_records(&records)?;
        let authorized_device = response
            .devices
            .iter()
            .find(|device| device.device_id == device_id)
            .cloned()
            .ok_or_else(|| DeviceAuthorityError::UnknownDevice(device_id.clone()))?;
        state.generated_devices.push(GeneratedDeviceRecord {
            _device_id: device_id.clone(),
            _identity: device,
            _encryption_private_key: device_encryption_private_key,
        });
        state.records = records.clone();
        drop(state);
        publish_authority_chain(daemon, &records).await?;

        Ok(DeviceAuthorityMutationResponse {
            identity: response.identity,
            latest_sequence: response.latest_sequence,
            device: authorized_device,
            devices: response.devices,
        })
    }

    pub async fn active_authorized_device_encryption_recipients(
        &self,
        daemon: &DaemonHandle,
    ) -> Result<Vec<EncryptedObjectRecipient>, DeviceAuthorityError> {
        self.ensure_bootstrapped(daemon).await?;
        let state = self.state.lock().await;
        let identity = state
            .records
            .first()
            .map(|record| record.body.identity.clone())
            .ok_or(DeviceAuthorityError::MissingLocalIdentity)?;
        let verified = verify_identity_authority_chain(&identity, &state.records)?;
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
        self.ensure_bootstrapped(daemon).await?;
        let operation = DeviceAuthorizationOperation::revoke_device(
            device_id.clone(),
            request.accepted_through_device_sequence,
            request.reason,
            unix_now(),
        );
        let record = self.append_signed_record(daemon, operation).await?;

        let mut state = self.state.lock().await;
        let mut records = state.records.clone();
        records.push(record);
        let response = response_from_records(&records)?;
        let device = response
            .devices
            .iter()
            .find(|device| device.device_id == device_id)
            .cloned()
            .ok_or_else(|| DeviceAuthorityError::UnknownDevice(device_id.clone()))?;
        state.records = records.clone();
        drop(state);
        publish_authority_chain(daemon, &records).await?;

        Ok(DeviceAuthorityMutationResponse {
            identity: response.identity,
            latest_sequence: response.latest_sequence,
            device,
            devices: response.devices,
        })
    }

    async fn ensure_bootstrapped(&self, daemon: &DaemonHandle) -> Result<(), DeviceAuthorityError> {
        if !self.state.lock().await.records.is_empty() {
            return Ok(());
        }

        let (identity, root_public_key) = local_identity_parts(daemon).await?;
        let issued_at = unix_now();
        let operation = DeviceAuthorizationOperation::authorize_device(
            LEGACY_ROOT_DEVICE_ID,
            root_public_key.clone(),
            default_device_capabilities(),
            Some("Legacy root device".to_string()),
            issued_at,
        );
        let body = DeviceAuthorizationRecordBody {
            record_type: "jolt.identity_authority_record".to_string(),
            version: 1,
            root_public_key,
            identity,
            sequence: 0,
            previous_record_hash: None,
            operation,
            issued_at,
        };
        let signature = daemon
            .sign_local_identity(body.canonical_bytes())
            .await
            .map_err(DeviceAuthorityError::Network)?;
        let record = DeviceAuthorizationRecord { body, signature };

        let mut state = self.state.lock().await;
        if state.records.is_empty() {
            state.records.push(record);
            let records = state.records.clone();
            drop(state);
            publish_authority_chain(daemon, &records).await?;
        }
        Ok(())
    }

    async fn append_signed_record(
        &self,
        daemon: &DaemonHandle,
        operation: DeviceAuthorizationOperation,
    ) -> Result<DeviceAuthorizationRecord, DeviceAuthorityError> {
        let (identity, root_public_key, sequence, previous_record_hash) = {
            let state = self.state.lock().await;
            let latest = state
                .records
                .last()
                .ok_or(DeviceAuthorityError::MissingLocalIdentity)?;
            (
                latest.body.identity.clone(),
                latest.body.root_public_key.clone(),
                latest.body.sequence + 1,
                Some(latest.record_hash()),
            )
        };
        let body = DeviceAuthorizationRecordBody {
            record_type: "jolt.identity_authority_record".to_string(),
            version: 1,
            root_public_key,
            identity,
            sequence,
            previous_record_hash,
            operation,
            issued_at: unix_now(),
        };
        let signature = daemon
            .sign_local_identity(body.canonical_bytes())
            .await
            .map_err(DeviceAuthorityError::Network)?;
        Ok(DeviceAuthorizationRecord { body, signature })
    }
}

async fn publish_authority_chain(
    daemon: &DaemonHandle,
    records: &[DeviceAuthorizationRecord],
) -> Result<(), DeviceAuthorityError> {
    let data = serde_json::to_vec(records)
        .map_err(|err| DeviceAuthorityError::InvalidLocalIdentity(err.to_string()))?;
    let temp_path = temp_authority_file_path();
    std::fs::write(&temp_path, data)
        .map_err(|err| DeviceAuthorityError::Network(NetworkError::Io(err)))?;
    let result = daemon
        .publish(temp_path.clone(), Some(IDENTITY_AUTHORITY_PATH.to_string()))
        .await;
    let _ = std::fs::remove_file(&temp_path);
    result?;
    Ok(())
}

fn temp_authority_file_path() -> PathBuf {
    let unique_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMP_AUTHORITY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "jolt_device_authority_{}_{unique_id}_{sequence}",
        std::process::id()
    ))
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

fn device_encryption_public_key(key: &IdentityEncryptionKey) -> DeviceEncryptionPublicKey {
    DeviceEncryptionPublicKey {
        key_id: key.key_id.clone(),
        suite_family: key.suite_family.clone(),
        public_key: key.public_key.clone(),
        created_at: key.created_at,
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

async fn local_identity_parts(
    daemon: &DaemonHandle,
) -> Result<(IdentityId, Vec<u8>), DeviceAuthorityError> {
    let address = match daemon.local_identity_address() {
        Some(address) => address.to_string(),
        None => daemon.status().await?.identity_address,
    };
    let address = JoltAddress::from_str(&address)
        .map_err(|err| DeviceAuthorityError::InvalidLocalIdentity(err.to_string()))?;
    let identity = address.identity().clone();
    let root_public_key = identity.as_public_key_bytes().to_vec();
    Ok((identity, root_public_key))
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
