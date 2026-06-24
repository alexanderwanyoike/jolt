use std::sync::Arc;

use jolt_identity::NodeIdentity;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LocalIdentityStore {
    state: Arc<Mutex<LocalIdentityState>>,
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
    },
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
}

impl LocalIdentityStore {
    pub fn new(default_identity: Option<String>) -> Self {
        let records = default_identity
            .iter()
            .map(|address| LocalIdentityRecord::Daemon {
                address: address.clone(),
                label: Some("Default".to_string()),
            })
            .collect();
        Self {
            state: Arc::new(Mutex::new(LocalIdentityState {
                records,
                active_identity: default_identity,
            })),
        }
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
    ) -> LocalIdentityView {
        let mut state = self.state.lock().await;
        let address = identity.jolt_address().to_string();
        if let Some(existing) = view_for_address(&state, &address) {
            return existing;
        }

        state
            .records
            .push(LocalIdentityRecord::Generated { identity, label });
        if state.active_identity.is_none() {
            state.active_identity = Some(address.clone());
        }
        view_for_address(&state, &address).expect("imported identity exists")
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
