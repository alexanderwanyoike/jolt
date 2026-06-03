use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppSessionStatus {
    Pending,
    Active,
    Rejected,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSessionRecord {
    pub request_id: String,
    pub session_id: Option<String>,
    pub app_id: String,
    pub app_name: String,
    pub app_origin: Option<String>,
    pub requested_identity: Option<String>,
    pub identity: Option<String>,
    pub requested_capabilities: Vec<String>,
    pub granted_capabilities: Vec<String>,
    pub status: AppSessionStatus,
    pub created_at: u64,
    pub approved_at: Option<u64>,
    pub rejected_at: Option<u64>,
    pub revoked_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub last_used_at: Option<u64>,
    pub token_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSessionView {
    pub request_id: String,
    pub session_id: Option<String>,
    pub app_id: String,
    pub app_name: String,
    pub app_origin: Option<String>,
    pub requested_identity: Option<String>,
    pub identity: Option<String>,
    pub requested_capabilities: Vec<String>,
    pub granted_capabilities: Vec<String>,
    pub status: AppSessionStatus,
    pub created_at: u64,
    pub approved_at: Option<u64>,
    pub rejected_at: Option<u64>,
    pub revoked_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub last_used_at: Option<u64>,
}

impl From<&AppSessionRecord> for AppSessionView {
    fn from(record: &AppSessionRecord) -> Self {
        Self {
            request_id: record.request_id.clone(),
            session_id: record.session_id.clone(),
            app_id: record.app_id.clone(),
            app_name: record.app_name.clone(),
            app_origin: record.app_origin.clone(),
            requested_identity: record.requested_identity.clone(),
            identity: record.identity.clone(),
            requested_capabilities: record.requested_capabilities.clone(),
            granted_capabilities: record.granted_capabilities.clone(),
            status: record.status.clone(),
            created_at: record.created_at,
            approved_at: record.approved_at,
            rejected_at: record.rejected_at,
            revoked_at: record.revoked_at,
            expires_at: record.expires_at,
            last_used_at: record.last_used_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppSessionRequest {
    pub app_id: String,
    pub app_name: String,
    pub app_origin: Option<String>,
    pub requested_identity: Option<String>,
    #[serde(default)]
    pub requested_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSessionRequestResponse {
    pub request_id: String,
    pub status: AppSessionStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApproveAppSessionRequest {
    pub identity: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproveAppSessionResponse {
    pub request_id: String,
    pub session_id: String,
    pub session_token: String,
    pub app_id: String,
    pub app_name: String,
    pub identity: String,
    pub capabilities: Vec<String>,
    pub status: AppSessionStatus,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppSessionStatusResponse {
    pub request_id: String,
    pub session_id: Option<String>,
    pub session_token: Option<String>,
    pub status: AppSessionStatus,
    pub identity: Option<String>,
    pub capabilities: Vec<String>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Error)]
pub enum AppSessionStoreError {
    #[error("missing app session bearer token")]
    MissingBearerToken,
    #[error("app session token is invalid or revoked")]
    InvalidToken,
    #[error("app session request not found: {0}")]
    RequestNotFound(String),
    #[error("app session not found: {0}")]
    SessionNotFound(String),
    #[error("app session request is not pending: {0}")]
    RequestNotPending(String),
    #[error("approved session requires an identity")]
    MissingIdentity,
    #[error("app session capability is not grantable: {0}")]
    CapabilityNotGrantable(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct AppSessionStore {
    path: PathBuf,
    state: Arc<Mutex<AppSessionState>>,
    issued_tokens: Arc<Mutex<HashMap<String, String>>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AppSessionState {
    records: Vec<AppSessionRecord>,
}

impl AppSessionStore {
    pub fn open_default() -> std::io::Result<Self> {
        Self::open(default_session_store_path())
    }

    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let state = load_state(&path)?;
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(state)),
            issued_tokens: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn create_request(
        &self,
        request: AppSessionRequest,
    ) -> Result<AppSessionRequestResponse, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let record = AppSessionRecord {
            request_id: new_id("req"),
            session_id: None,
            app_id: request.app_id,
            app_name: request.app_name,
            app_origin: request.app_origin,
            requested_identity: request.requested_identity,
            identity: None,
            requested_capabilities: request.requested_capabilities,
            granted_capabilities: Vec::new(),
            status: AppSessionStatus::Pending,
            created_at: now_secs(),
            approved_at: None,
            rejected_at: None,
            revoked_at: None,
            expires_at: None,
            last_used_at: None,
            token_hash: None,
        };
        let response = AppSessionRequestResponse {
            request_id: record.request_id.clone(),
            status: record.status.clone(),
        };
        state.records.push(record);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn list_requests(&self) -> Vec<AppSessionView> {
        let state = self.state.lock().await;
        state
            .records
            .iter()
            .filter(|record| record.status == AppSessionStatus::Pending)
            .map(AppSessionView::from)
            .collect()
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
        approval: ApproveAppSessionRequest,
    ) -> Result<ApproveAppSessionResponse, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .iter_mut()
            .find(|record| record.request_id == request_id)
            .ok_or_else(|| AppSessionStoreError::RequestNotFound(request_id.to_string()))?;

        if record.status != AppSessionStatus::Pending {
            return Err(AppSessionStoreError::RequestNotPending(
                request_id.to_string(),
            ));
        }

        let identity = approval
            .identity
            .or_else(|| record.requested_identity.clone())
            .ok_or(AppSessionStoreError::MissingIdentity)?;
        let capabilities = if approval.capabilities.is_empty() {
            record.requested_capabilities.clone()
        } else {
            approval.capabilities
        };
        for capability in &capabilities {
            if !is_grantable_app_capability(capability) {
                return Err(AppSessionStoreError::CapabilityNotGrantable(
                    capability.clone(),
                ));
            }
        }
        let session_id = new_id("sess");
        let session_token = new_token();

        record.session_id = Some(session_id.clone());
        record.identity = Some(identity.clone());
        record.granted_capabilities = capabilities.clone();
        record.status = AppSessionStatus::Active;
        record.approved_at = Some(now_secs());
        record.expires_at = approval.expires_at;
        record.token_hash = Some(hash_token(&session_token));

        let response = ApproveAppSessionResponse {
            request_id: record.request_id.clone(),
            session_id,
            session_token,
            app_id: record.app_id.clone(),
            app_name: record.app_name.clone(),
            identity,
            capabilities,
            status: record.status.clone(),
            expires_at: record.expires_at,
        };

        save_state(&self.path, &state)?;
        self.issued_tokens
            .lock()
            .await
            .insert(request_id.to_string(), response.session_token.clone());
        Ok(response)
    }

    pub async fn request_status(
        &self,
        request_id: &str,
    ) -> Result<AppSessionStatusResponse, AppSessionStoreError> {
        let state = self.state.lock().await;
        let record = state
            .records
            .iter()
            .find(|record| record.request_id == request_id)
            .ok_or_else(|| AppSessionStoreError::RequestNotFound(request_id.to_string()))?;
        let session_token = if record.status == AppSessionStatus::Active {
            self.issued_tokens.lock().await.get(request_id).cloned()
        } else {
            None
        };
        Ok(AppSessionStatusResponse {
            request_id: record.request_id.clone(),
            session_id: record.session_id.clone(),
            session_token,
            status: record.status.clone(),
            identity: record.identity.clone(),
            capabilities: record.granted_capabilities.clone(),
            expires_at: record.expires_at,
        })
    }

    pub async fn list_sessions(&self) -> Vec<AppSessionView> {
        let state = self.state.lock().await;
        state
            .records
            .iter()
            .filter(|record| record.session_id.is_some())
            .map(AppSessionView::from)
            .collect()
    }

    pub async fn reject_request(
        &self,
        request_id: &str,
    ) -> Result<AppSessionView, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .iter_mut()
            .find(|record| record.request_id == request_id)
            .ok_or_else(|| AppSessionStoreError::RequestNotFound(request_id.to_string()))?;

        if record.status != AppSessionStatus::Pending {
            return Err(AppSessionStoreError::RequestNotPending(
                request_id.to_string(),
            ));
        }

        record.status = AppSessionStatus::Rejected;
        record.rejected_at = Some(now_secs());
        let response = AppSessionView::from(&*record);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn revoke_session(
        &self,
        session_id: &str,
    ) -> Result<AppSessionView, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .iter_mut()
            .find(|record| record.session_id.as_deref() == Some(session_id))
            .ok_or_else(|| AppSessionStoreError::SessionNotFound(session_id.to_string()))?;

        record.status = AppSessionStatus::Revoked;
        record.revoked_at = Some(now_secs());
        let response = AppSessionView::from(&*record);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn session_for_token(
        &self,
        token: &str,
    ) -> Result<AppSessionView, AppSessionStoreError> {
        let token_hash = hash_token(token);
        let mut state = self.state.lock().await;
        let now = now_secs();
        let record = state
            .records
            .iter_mut()
            .find(|record| record.token_hash.as_deref() == Some(token_hash.as_str()))
            .ok_or(AppSessionStoreError::InvalidToken)?;

        if record.status != AppSessionStatus::Active {
            return Err(AppSessionStoreError::InvalidToken);
        }

        if record
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
        {
            record.status = AppSessionStatus::Expired;
            save_state(&self.path, &state)?;
            return Err(AppSessionStoreError::InvalidToken);
        }

        record.last_used_at = Some(now);
        let response = AppSessionView::from(&*record);
        save_state(&self.path, &state)?;
        Ok(response)
    }
}

fn load_state(path: &Path) -> std::io::Result<AppSessionState> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(std::io::Error::other),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AppSessionState::default()),
        Err(err) => Err(err),
    }
}

fn is_grantable_app_capability(capability: &str) -> bool {
    capability == "resolve:public"
        || capability == "fetch:public"
        || is_grantable_path_capability(capability, "publish:")
        || is_grantable_path_capability(capability, "inventory:")
        || is_grantable_path_capability(capability, "pin:own:")
}

fn is_grantable_path_capability(capability: &str, prefix: &str) -> bool {
    let Some(scope) = capability.strip_prefix(prefix) else {
        return false;
    };

    scope.starts_with('/') && !scope.contains("..")
}

fn save_state(path: &Path, state: &AppSessionState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(state).map_err(std::io::Error::other)?;
    std::fs::write(path, raw)
}

fn default_session_store_path() -> PathBuf {
    directories::ProjectDirs::from("net", "jolt", "jolt")
        .map(|dirs| dirs.data_dir().join("app-sessions.json"))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".jolt").join("app-sessions.json")
        })
}

fn new_id(prefix: &str) -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}_{}", hex::encode(bytes))
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("jolt_app_{}", hex::encode(bytes))
}

fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
