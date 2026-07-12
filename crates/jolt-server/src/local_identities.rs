use std::sync::Arc;

use jolt_identity::ExportedIdentityEncryptionKeypair;
use jolt_identity::NodeIdentity;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LocalIdentityStore {
    state: Arc<Mutex<LocalIdentityState>>,
    storage_dir: PathBuf,
}

struct LocalIdentityState {
    records: Vec<LocalIdentityRecord>,
    active_identity: Option<String>,
}

enum LocalIdentityRecord {
    Daemon {
        address: String,
        label: Option<String>,
    },
    Generated {
        identity: NodeIdentity,
        label: Option<String>,
        encryption_keypairs: Vec<ExportedIdentityEncryptionKeypair>,
    },
}

#[derive(Serialize, Deserialize)]
struct PersistedLocalIdentity {
    label: Option<String>,
    encryption_keypairs: Vec<ExportedIdentityEncryptionKeypair>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalIdentityView {
    pub address: String,
    pub label: Option<String>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LocalIdentitiesResponse {
    pub active_identity: Option<String>,
    pub identities: Vec<LocalIdentityView>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateLocalIdentityRequest {
    pub label: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectLocalIdentityRequest {
    pub identity: String,
}

#[derive(Debug, Error)]
pub enum LocalIdentityError {
    #[error("unknown local identity: {0}")]
    UnknownIdentity(String),
    #[error("local identity is required")]
    MissingIdentity,
    #[error("cannot delete daemon signing identity: {0}")]
    ProtectedIdentity(String),
    #[error("local identity storage error: {0}")]
    Storage(String),
}

impl LocalIdentityStore {
    pub fn open(
        default_identity: Option<String>,
        storage_dir: PathBuf,
    ) -> Result<Self, LocalIdentityError> {
        let mut records: Vec<_> = default_identity
            .iter()
            .map(|address| LocalIdentityRecord::Daemon {
                address: address.clone(),
                label: Some("Default".to_string()),
            })
            .collect();
        records.extend(load_generated_identities(&storage_dir)?);
        Ok(Self {
            state: Arc::new(Mutex::new(LocalIdentityState {
                records,
                active_identity: default_identity,
            })),
            storage_dir,
        })
    }

    pub async fn list(&self) -> LocalIdentitiesResponse {
        let state = self.state.lock().await;
        response_from_state(&state)
    }

    pub async fn active_identity(&self) -> Option<String> {
        self.state.lock().await.active_identity.clone()
    }

    pub async fn is_daemon_identity(&self, identity: &str) -> bool {
        self.state.lock().await.records.iter().any(|record| {
            matches!(record, LocalIdentityRecord::Daemon { .. }) && record.address() == identity
        })
    }

    pub async fn generated_identity(
        &self,
        identity: &str,
    ) -> Result<Option<NodeIdentity>, LocalIdentityError> {
        let identity = identity.trim();
        if identity.is_empty() {
            return Err(LocalIdentityError::MissingIdentity);
        }

        let state = self.state.lock().await;
        let record = state
            .records
            .iter()
            .find(|record| record.address() == identity)
            .ok_or_else(|| LocalIdentityError::UnknownIdentity(identity.to_string()))?;

        Ok(match record {
            LocalIdentityRecord::Daemon { .. } => None,
            LocalIdentityRecord::Generated { identity, .. } => Some(clone_node_identity(identity)),
        })
    }

    pub async fn contains(&self, identity: &str) -> bool {
        self.state
            .lock()
            .await
            .records
            .iter()
            .any(|record| record.address() == identity)
    }

    pub async fn create(
        &self,
        request: CreateLocalIdentityRequest,
    ) -> Result<LocalIdentityView, LocalIdentityError> {
        let mut state = self.state.lock().await;
        let identity = NodeIdentity::generate();
        let address = identity.jolt_address().to_string();
        state.records.push(LocalIdentityRecord::Generated {
            identity,
            label: request.label,
            encryption_keypairs: Vec::new(),
        });
        if state.active_identity.is_none() {
            state.active_identity = Some(address.clone());
        }
        Ok(view_for_address(&state, &address).expect("created identity exists"))
    }

    pub async fn import_recovered(
        &self,
        identity: NodeIdentity,
        label: Option<String>,
        encryption_keypairs: Vec<ExportedIdentityEncryptionKeypair>,
    ) -> Result<LocalIdentityView, LocalIdentityError> {
        let mut state = self.state.lock().await;
        let address = identity.jolt_address().to_string();
        if let Some(existing) = view_for_address(&state, &address) {
            return Ok(existing);
        }

        persist_generated_identity(
            &self.storage_dir,
            &identity,
            label.clone(),
            &encryption_keypairs,
        )?;
        state.records.push(LocalIdentityRecord::Generated {
            identity,
            label,
            encryption_keypairs,
        });
        if state.active_identity.is_none() {
            state.active_identity = Some(address.clone());
        }
        Ok(view_for_address(&state, &address).expect("imported identity exists"))
    }

    pub async fn generated_identity_encryption_keypairs(
        &self,
        identity: &str,
    ) -> Result<Vec<ExportedIdentityEncryptionKeypair>, LocalIdentityError> {
        let state = self.state.lock().await;
        let record = state
            .records
            .iter()
            .find(|record| record.address() == identity)
            .ok_or_else(|| LocalIdentityError::UnknownIdentity(identity.to_string()))?;
        Ok(match record {
            LocalIdentityRecord::Daemon { .. } => Vec::new(),
            LocalIdentityRecord::Generated {
                encryption_keypairs,
                ..
            } => encryption_keypairs.clone(),
        })
    }

    pub async fn select(
        &self,
        request: SelectLocalIdentityRequest,
    ) -> Result<LocalIdentitiesResponse, LocalIdentityError> {
        if request.identity.trim().is_empty() {
            return Err(LocalIdentityError::MissingIdentity);
        }

        let mut state = self.state.lock().await;
        let exists = state
            .records
            .iter()
            .any(|record| record.address() == request.identity);
        if !exists {
            return Err(LocalIdentityError::UnknownIdentity(request.identity));
        }
        state.active_identity = Some(request.identity);
        Ok(response_from_state(&state))
    }

    pub async fn delete(
        &self,
        identity: String,
    ) -> Result<LocalIdentitiesResponse, LocalIdentityError> {
        let identity = identity.trim();
        if identity.is_empty() {
            return Err(LocalIdentityError::MissingIdentity);
        }

        let mut state = self.state.lock().await;
        let index = state
            .records
            .iter()
            .position(|record| record.address() == identity)
            .ok_or_else(|| LocalIdentityError::UnknownIdentity(identity.to_string()))?;

        if matches!(state.records[index], LocalIdentityRecord::Daemon { .. }) {
            return Err(LocalIdentityError::ProtectedIdentity(identity.to_string()));
        }

        let was_active = state.active_identity.as_deref() == Some(identity);
        state.records.remove(index);
        if was_active {
            state.active_identity = fallback_active_identity(&state);
        }
        Ok(response_from_state(&state))
    }
}

fn persist_generated_identity(
    storage_dir: &Path,
    identity: &NodeIdentity,
    label: Option<String>,
    encryption_keypairs: &[ExportedIdentityEncryptionKeypair],
) -> Result<(), LocalIdentityError> {
    let identity_dir = storage_dir.join(identity.identity_id().to_string());
    identity
        .save(&identity_dir)
        .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
    let metadata = serde_json::to_vec_pretty(&PersistedLocalIdentity {
        label,
        encryption_keypairs: encryption_keypairs.to_vec(),
    })
    .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
    let metadata_path = identity_dir.join("recovery.json");
    std::fs::write(&metadata_path, metadata)
        .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&metadata_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
    }
    Ok(())
}

fn load_generated_identities(
    storage_dir: &Path,
) -> Result<Vec<LocalIdentityRecord>, LocalIdentityError> {
    if !storage_dir.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in std::fs::read_dir(storage_dir)
        .map_err(|error| LocalIdentityError::Storage(error.to_string()))?
    {
        let path = entry
            .map_err(|error| LocalIdentityError::Storage(error.to_string()))?
            .path();
        if !path.is_dir() {
            continue;
        }
        let identity = NodeIdentity::load(&path)
            .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
        let metadata: PersistedLocalIdentity = serde_json::from_slice(
            &std::fs::read(path.join("recovery.json"))
                .map_err(|error| LocalIdentityError::Storage(error.to_string()))?,
        )
        .map_err(|error| LocalIdentityError::Storage(error.to_string()))?;
        records.push(LocalIdentityRecord::Generated {
            identity,
            label: metadata.label,
            encryption_keypairs: metadata.encryption_keypairs,
        });
    }
    Ok(records)
}

impl LocalIdentityRecord {
    fn address(&self) -> String {
        match self {
            Self::Daemon { address, .. } => address.clone(),
            Self::Generated { identity, .. } => identity.jolt_address().to_string(),
        }
    }

    fn label(&self) -> Option<String> {
        match self {
            Self::Daemon { label, .. } | Self::Generated { label, .. } => label.clone(),
        }
    }
}

fn response_from_state(state: &LocalIdentityState) -> LocalIdentitiesResponse {
    LocalIdentitiesResponse {
        active_identity: state.active_identity.clone(),
        identities: state
            .records
            .iter()
            .map(|record| {
                let address = record.address();
                LocalIdentityView {
                    active: state.active_identity.as_deref() == Some(address.as_str()),
                    address,
                    label: record.label(),
                }
            })
            .collect(),
    }
}

fn fallback_active_identity(state: &LocalIdentityState) -> Option<String> {
    state
        .records
        .iter()
        .find(|record| matches!(record, LocalIdentityRecord::Daemon { .. }))
        .or_else(|| state.records.first())
        .map(LocalIdentityRecord::address)
}

fn view_for_address(state: &LocalIdentityState, address: &str) -> Option<LocalIdentityView> {
    state.records.iter().find_map(|record| {
        let record_address = record.address();
        (record_address == address).then(|| LocalIdentityView {
            active: state.active_identity.as_deref() == Some(record_address.as_str()),
            address: record_address,
            label: record.label(),
        })
    })
}

fn clone_node_identity(identity: &NodeIdentity) -> NodeIdentity {
    NodeIdentity::from_signing_key_bytes(&identity.signing_key_bytes())
        .expect("stored generated identity has valid signing key bytes")
}
