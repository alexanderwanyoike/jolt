use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use jolt_core::IdentityId;
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
    #[serde(default)]
    pub device_id: Option<String>,
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
    pub device_id: Option<String>,
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
            device_id: record.device_id.clone(),
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
    pub device_id: String,
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

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataSubscriptionLifecycle {
    Active,
    Dormant,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DataSubscriptionRefresh {
    Loading,
    Updating {
        #[serde(skip_serializing_if = "Option::is_none")]
        last_verified_at: Option<u64>,
    },
    Ready {
        last_verified_at: u64,
    },
    Stale {
        last_verified_at: u64,
        reason: String,
    },
    Unavailable {
        reason: String,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct DataSubscriptionRecord {
    pub id: String,
    pub session_id: String,
    pub identity: String,
    pub prefix: String,
    pub lifecycle: DataSubscriptionLifecycle,
    pub refresh: DataSubscriptionRefresh,
    pub created_at: u64,
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
    #[error("app session capability was not requested: {0}")]
    CapabilityNotRequested(String),
    #[error("data subscription capacity exceeded")]
    DataSubscriptionCapacityExceeded,
    #[error("data subscription not found: {0}")]
    DataSubscriptionNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct AppSessionStore {
    path: PathBuf,
    state: Arc<Mutex<AppSessionState>>,
    issued_tokens: Arc<Mutex<HashMap<String, String>>>,
    max_data_subscriptions: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AppSessionState {
    records: Vec<AppSessionRecord>,
    #[serde(default)]
    data_subscriptions: Vec<DataSubscriptionRecord>,
}

const DEFAULT_MAX_DATA_SUBSCRIPTIONS: usize = 4_096;

impl AppSessionStore {
    pub fn open_default() -> std::io::Result<Self> {
        Self::open(default_session_store_path())
    }

    pub fn open(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        Self::open_with_data_subscription_limit(path, DEFAULT_MAX_DATA_SUBSCRIPTIONS)
    }

    pub fn open_with_data_subscription_limit(
        path: impl Into<PathBuf>,
        max_data_subscriptions: usize,
    ) -> std::io::Result<Self> {
        let path = path.into();
        let state = load_state(&path)?;
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(state)),
            issued_tokens: Arc::new(Mutex::new(HashMap::new())),
            max_data_subscriptions,
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
            device_id: None,
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

    pub async fn list_requests_for_identity(&self, identity: &str) -> Vec<AppSessionView> {
        let state = self.state.lock().await;
        state
            .records
            .iter()
            .filter(|record| {
                matches!(
                    record.status,
                    AppSessionStatus::Pending | AppSessionStatus::Rejected
                )
            })
            .filter(|record| record.requested_identity.as_deref() == Some(identity))
            .map(AppSessionView::from)
            .collect()
    }

    pub async fn requested_capabilities(
        &self,
        request_id: &str,
    ) -> Result<Vec<String>, AppSessionStoreError> {
        let state = self.state.lock().await;
        let record = state
            .records
            .iter()
            .find(|record| record.request_id == request_id)
            .ok_or_else(|| AppSessionStoreError::RequestNotFound(request_id.to_string()))?;
        if record.status != AppSessionStatus::Pending {
            return Err(AppSessionStoreError::RequestNotPending(
                request_id.to_string(),
            ));
        }
        Ok(record.requested_capabilities.clone())
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
        approval: ApproveAppSessionRequest,
        local_device_id: &str,
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
            if !capability_was_requested(capability, &record.requested_capabilities) {
                return Err(AppSessionStoreError::CapabilityNotRequested(
                    capability.clone(),
                ));
            }
        }
        let session_id = new_id("sess");
        let session_token = new_token();

        record.session_id = Some(session_id.clone());
        record.identity = Some(identity.clone());
        record.device_id = Some(local_device_id.to_string());
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
            device_id: local_device_id.to_string(),
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

    pub async fn list_sessions_for_identity(&self, identity: &str) -> Vec<AppSessionView> {
        let state = self.state.lock().await;
        state
            .records
            .iter()
            .filter(|record| record.session_id.is_some())
            .filter(|record| record.identity.as_deref() == Some(identity))
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
        state
            .data_subscriptions
            .retain(|subscription| subscription.session_id != session_id);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn revoke_session_for_identity(
        &self,
        session_id: &str,
        identity: &str,
    ) -> Result<AppSessionView, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let record = state
            .records
            .iter_mut()
            .find(|record| {
                record.session_id.as_deref() == Some(session_id)
                    && record.identity.as_deref() == Some(identity)
            })
            .ok_or_else(|| AppSessionStoreError::SessionNotFound(session_id.to_string()))?;

        record.status = AppSessionStatus::Revoked;
        record.revoked_at = Some(now_secs());
        let response = AppSessionView::from(&*record);
        state
            .data_subscriptions
            .retain(|subscription| subscription.session_id != session_id);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn revoke_sessions_for_device(
        &self,
        device_id: &str,
    ) -> Result<Vec<AppSessionView>, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let now = now_secs();
        let mut revoked = Vec::new();
        for record in state.records.iter_mut().filter(|record| {
            record.session_id.is_some()
                && record.device_id.as_deref() == Some(device_id)
                && record.status == AppSessionStatus::Active
        }) {
            record.status = AppSessionStatus::Revoked;
            record.revoked_at = Some(now);
            revoked.push(AppSessionView::from(&*record));
        }
        if !revoked.is_empty() {
            let revoked_session_ids = revoked
                .iter()
                .filter_map(|session| session.session_id.as_deref())
                .collect::<Vec<_>>();
            state.data_subscriptions.retain(|subscription| {
                !revoked_session_ids.contains(&subscription.session_id.as_str())
            });
            save_state(&self.path, &state)?;
        }
        Ok(revoked)
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
            let session_id = record.session_id.clone();
            if let Some(session_id) = session_id {
                state
                    .data_subscriptions
                    .retain(|subscription| subscription.session_id != session_id);
            }
            save_state(&self.path, &state)?;
            return Err(AppSessionStoreError::InvalidToken);
        }

        record.last_used_at = Some(now);
        let response = AppSessionView::from(&*record);
        save_state(&self.path, &state)?;
        Ok(response)
    }

    pub async fn register_data_subscription(
        &self,
        session_id: &str,
        identity: String,
        prefix: String,
    ) -> Result<DataSubscriptionRecord, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        if let Some(existing) = state.data_subscriptions.iter().find(|subscription| {
            subscription.session_id == session_id
                && subscription.identity == identity
                && subscription.prefix == prefix
        }) {
            return Ok(existing.clone());
        }
        if state.data_subscriptions.len() >= self.max_data_subscriptions {
            return Err(AppSessionStoreError::DataSubscriptionCapacityExceeded);
        }
        let subscription = DataSubscriptionRecord {
            id: new_id("sub"),
            session_id: session_id.to_string(),
            identity,
            prefix,
            lifecycle: DataSubscriptionLifecycle::Dormant,
            refresh: DataSubscriptionRefresh::Loading,
            created_at: now_secs(),
        };
        state.data_subscriptions.push(subscription.clone());
        save_state(&self.path, &state)?;
        Ok(subscription)
    }

    pub async fn list_data_subscriptions(&self, session_id: &str) -> Vec<DataSubscriptionRecord> {
        let state = self.state.lock().await;
        state
            .data_subscriptions
            .iter()
            .filter(|subscription| subscription.session_id == session_id)
            .cloned()
            .collect()
    }

    pub async fn data_subscription(
        &self,
        session_id: &str,
        subscription_id: &str,
    ) -> Result<DataSubscriptionRecord, AppSessionStoreError> {
        let state = self.state.lock().await;
        state
            .data_subscriptions
            .iter()
            .find(|subscription| {
                subscription.session_id == session_id && subscription.id == subscription_id
            })
            .cloned()
            .ok_or_else(|| {
                AppSessionStoreError::DataSubscriptionNotFound(subscription_id.to_string())
            })
    }

    pub async fn update_data_subscription_state(
        &self,
        session_id: &str,
        subscription_id: &str,
        lifecycle: DataSubscriptionLifecycle,
        refresh: DataSubscriptionRefresh,
    ) -> Result<DataSubscriptionRecord, AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let subscription = state
            .data_subscriptions
            .iter_mut()
            .find(|subscription| {
                subscription.session_id == session_id && subscription.id == subscription_id
            })
            .ok_or_else(|| {
                AppSessionStoreError::DataSubscriptionNotFound(subscription_id.to_string())
            })?;
        subscription.lifecycle = lifecycle;
        subscription.refresh = refresh;
        let updated = subscription.clone();
        save_state(&self.path, &state)?;
        Ok(updated)
    }

    pub async fn remove_data_subscription(
        &self,
        session_id: &str,
        subscription_id: &str,
    ) -> Result<(), AppSessionStoreError> {
        let mut state = self.state.lock().await;
        let index = state
            .data_subscriptions
            .iter()
            .position(|subscription| {
                subscription.session_id == session_id && subscription.id == subscription_id
            })
            .ok_or_else(|| {
                AppSessionStoreError::DataSubscriptionNotFound(subscription_id.to_string())
            })?;
        state.data_subscriptions.remove(index);
        save_state(&self.path, &state)?;
        Ok(())
    }
}

fn load_state(path: &Path) -> std::io::Result<AppSessionState> {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(std::io::Error::other),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AppSessionState::default()),
        Err(err) => Err(err),
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum AppCapability {
    ResolvePublic,
    FetchPublic,
    IngressSend,
    IngressRead,
    IngressDecide,
    Enumerate {
        identity_scope: EnumerateIdentityScope,
        scope: PathScope,
    },
    Subscribe {
        identity_scope: SubscriptionIdentityScope,
        scope: PathScope,
    },
    Path {
        action: PathCapabilityAction,
        scope: PathScope,
    },
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum EnumerateIdentityScope {
    SelfIdentity,
    AnyIdentity,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum SubscriptionIdentityScope {
    AnyIdentity,
    Exact(IdentityId),
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum PathCapabilityAction {
    Publish,
    Delete,
    PublishEncrypted,
    Inventory,
    PinOwn,
    Encrypt,
    Decrypt,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum PathScope {
    Exact(String),
    Prefix(String),
}

impl AppCapability {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "resolve:public" => Some(Self::ResolvePublic),
            "fetch:public" => Some(Self::FetchPublic),
            "ingress:send" => Some(Self::IngressSend),
            "ingress:read" => Some(Self::IngressRead),
            "ingress:decide" => Some(Self::IngressDecide),
            _ if raw.starts_with("enumerate:self:") => Some(Self::Enumerate {
                identity_scope: EnumerateIdentityScope::SelfIdentity,
                scope: PathScope::parse(raw.strip_prefix("enumerate:self:")?)?,
            }),
            _ if raw.starts_with("enumerate:any:") => Some(Self::Enumerate {
                identity_scope: EnumerateIdentityScope::AnyIdentity,
                scope: PathScope::parse(raw.strip_prefix("enumerate:any:")?)?,
            }),
            _ if raw.starts_with("subscribe:") => {
                let (identity, scope) = raw.strip_prefix("subscribe:")?.split_once(':')?;
                let identity_scope = if identity == "any" {
                    SubscriptionIdentityScope::AnyIdentity
                } else {
                    SubscriptionIdentityScope::Exact(
                        IdentityId::from_str(identity.trim_end_matches(".jolt")).ok()?,
                    )
                };
                Some(Self::Subscribe {
                    identity_scope,
                    scope: PathScope::parse(scope)?,
                })
            }
            _ => parse_path_capability(raw),
        }
    }

    fn contains(&self, granted: &Self) -> bool {
        match (self, granted) {
            (Self::ResolvePublic, Self::ResolvePublic)
            | (Self::FetchPublic, Self::FetchPublic)
            | (Self::IngressSend, Self::IngressSend)
            | (Self::IngressRead, Self::IngressRead)
            | (Self::IngressDecide, Self::IngressDecide) => true,
            (
                Self::Enumerate {
                    identity_scope: requested_identity_scope,
                    scope: requested_scope,
                },
                Self::Enumerate {
                    identity_scope: granted_identity_scope,
                    scope: granted_scope,
                },
            ) if requested_identity_scope == granted_identity_scope
                || *requested_identity_scope == EnumerateIdentityScope::AnyIdentity =>
            {
                requested_scope.contains(granted_scope)
            }
            (
                Self::Subscribe {
                    identity_scope: requested_identity_scope,
                    scope: requested_scope,
                },
                Self::Subscribe {
                    identity_scope: granted_identity_scope,
                    scope: granted_scope,
                },
            ) if requested_identity_scope == granted_identity_scope
                || *requested_identity_scope == SubscriptionIdentityScope::AnyIdentity =>
            {
                requested_scope.contains(granted_scope)
            }
            (
                Self::Path {
                    action: requested_action,
                    scope: requested_scope,
                },
                Self::Path {
                    action: granted_action,
                    scope: granted_scope,
                },
            ) if requested_action == granted_action => requested_scope.contains(granted_scope),
            _ => false,
        }
    }
}

impl PathScope {
    fn parse(raw: &str) -> Option<Self> {
        if !raw.starts_with('/')
            || raw.contains('?')
            || raw.contains('#')
            || raw.chars().any(char::is_whitespace)
        {
            return None;
        }

        let wildcard_count = raw.chars().filter(|ch| *ch == '*').count();
        if wildcard_count > 1 {
            return None;
        }
        if wildcard_count == 1 {
            let base = raw.strip_suffix("/*")?;
            if !is_valid_exact_scope_base(base) {
                return None;
            }
            return Some(Self::Prefix(format!("{base}/")));
        }

        if !is_valid_exact_scope_base(raw) {
            return None;
        }
        Some(Self::Exact(raw.to_string()))
    }

    fn contains(&self, granted: &Self) -> bool {
        match (self, granted) {
            (Self::Exact(requested), Self::Exact(granted)) => requested == granted,
            (Self::Prefix(prefix), Self::Exact(granted)) => granted.starts_with(prefix),
            (Self::Prefix(requested), Self::Prefix(granted)) => granted.starts_with(requested),
            (Self::Exact(_), Self::Prefix(_)) => false,
        }
    }
}

fn parse_path_capability(raw: &str) -> Option<AppCapability> {
    for (prefix, action) in [
        ("publish:encrypted:", PathCapabilityAction::PublishEncrypted),
        ("publish:", PathCapabilityAction::Publish),
        ("delete:", PathCapabilityAction::Delete),
        ("inventory:", PathCapabilityAction::Inventory),
        ("pin:own:", PathCapabilityAction::PinOwn),
        ("encrypt:", PathCapabilityAction::Encrypt),
        ("decrypt:", PathCapabilityAction::Decrypt),
    ] {
        let Some(scope) = raw.strip_prefix(prefix) else {
            continue;
        };
        return Some(AppCapability::Path {
            action,
            scope: PathScope::parse(scope)?,
        });
    }

    None
}

fn is_valid_exact_scope_base(scope: &str) -> bool {
    scope != "/"
        && scope
            .split('/')
            .filter(|segment| !segment.is_empty())
            .all(|segment| segment != "." && segment != "..")
}

fn is_grantable_app_capability(capability: &str) -> bool {
    AppCapability::parse(capability).is_some()
}

fn capability_was_requested(capability: &str, requested: &[String]) -> bool {
    let Some(granted) = AppCapability::parse(capability) else {
        return false;
    };
    requested
        .iter()
        .filter_map(|raw| AppCapability::parse(raw))
        .any(|requested| requested.contains(&granted))
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

#[cfg(test)]
mod tests {
    use super::*;

    async fn approved_session(
        store: &AppSessionStore,
        expires_at: Option<u64>,
    ) -> ApproveAppSessionResponse {
        let remote_identity = format!("{}.jolt", IdentityId::from_public_key([7; 32]),);
        let request = store
            .create_request(AppSessionRequest {
                app_id: "chirp.local".to_string(),
                app_name: "Chirp".to_string(),
                app_origin: None,
                requested_identity: Some("local.jolt".to_string()),
                requested_capabilities: vec![format!("subscribe:{remote_identity}:/chirp/posts/*")],
            })
            .await
            .unwrap();
        store
            .approve_request(
                &request.request_id,
                ApproveAppSessionRequest {
                    identity: None,
                    capabilities: Vec::new(),
                    expires_at,
                },
                "dev_local",
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn configured_data_subscription_limit_rejects_without_evicting() {
        let dir = tempfile::tempdir().unwrap();
        let store =
            AppSessionStore::open_with_data_subscription_limit(dir.path().join("sessions.json"), 1)
                .unwrap();

        let first = store
            .register_data_subscription(
                "session_a",
                "alice.jolt".to_string(),
                "/chirp/posts/".to_string(),
            )
            .await
            .unwrap();
        let error = store
            .register_data_subscription(
                "session_b",
                "bob.jolt".to_string(),
                "/chirp/posts/".to_string(),
            )
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            AppSessionStoreError::DataSubscriptionCapacityExceeded
        ));
        assert_eq!(
            store.list_data_subscriptions("session_a").await,
            vec![first]
        );
        assert!(store.list_data_subscriptions("session_b").await.is_empty());
    }

    #[tokio::test]
    async fn expiry_and_revocation_remove_durable_data_subscription_intent() {
        for revoke in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("sessions.json");
            let store = AppSessionStore::open(&path).unwrap();
            let session = approved_session(&store, (!revoke).then_some(0)).await;
            store
                .register_data_subscription(
                    &session.session_id,
                    "alice.jolt".to_string(),
                    "/chirp/posts/".to_string(),
                )
                .await
                .unwrap();

            if revoke {
                store.revoke_session(&session.session_id).await.unwrap();
            } else {
                assert!(matches!(
                    store.session_for_token(&session.session_token).await,
                    Err(AppSessionStoreError::InvalidToken)
                ));
            }

            let reopened = AppSessionStore::open(&path).unwrap();
            assert!(reopened
                .list_data_subscriptions(&session.session_id)
                .await
                .is_empty());
        }
    }
}
