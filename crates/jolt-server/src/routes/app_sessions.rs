use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use jolt_network::NetworkError;
use serde_json::json;

use crate::session_store::{AppSessionRequest, AppSessionStoreError, ApproveAppSessionRequest};
use crate::state::AppState;

pub async fn request_session(
    State(state): State<AppState>,
    Json(mut request): Json<AppSessionRequest>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    if request.requested_identity.is_none() {
        request.requested_identity = state.local_identities.active_identity().await;
    }
    let response = state.sessions.create_request(request).await?;
    Ok(Json(response))
}

pub async fn get_request_status(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.request_status(&request_id).await?;
    Ok(Json(response))
}

pub async fn current_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let token = bearer_token(&headers)?.to_string();
    let response = state.sessions.session_for_token(&token).await?;
    Ok(Json(response))
}

pub async fn list_requests(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let identity = state
        .local_identities
        .active_identity()
        .await
        .ok_or(AppSessionStoreError::MissingIdentity)?;
    Ok(Json(
        state.sessions.list_requests_for_identity(&identity).await,
    ))
}

pub async fn approve_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(mut request): Json<ApproveAppSessionRequest>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    if request.identity.is_none() {
        request.identity = state.local_identities.active_identity().await;
    }
    let effective_capabilities = if request.capabilities.is_empty() {
        state.sessions.requested_capabilities(&request_id).await?
    } else {
        request.capabilities.clone()
    };
    if capabilities_require_daemon_identity(&effective_capabilities) {
        let identity = request
            .identity
            .as_deref()
            .ok_or(AppSessionApiError::Store(
                AppSessionStoreError::MissingIdentity,
            ))?;
        if !identity_can_receive_daemon_capabilities(&state, identity).await? {
            return Err(AppSessionApiError::LocalIdentityCapability {
                identity: identity.to_string(),
            });
        }
    }
    let local_device_id = state.daemon.status().await?.local_device_id;
    let response = state
        .sessions
        .approve_request(&request_id, request, &local_device_id)
        .await?;
    if grants_private_content_authority(&response.capabilities) {
        state.daemon.ensure_local_identity_encryption_key().await?;
    }
    Ok(Json(response))
}

pub async fn reject_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.reject_request(&request_id).await?;
    Ok(Json(response))
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let identity = state
        .local_identities
        .active_identity()
        .await
        .ok_or(AppSessionStoreError::MissingIdentity)?;
    Ok(Json(
        state.sessions.list_sessions_for_identity(&identity).await,
    ))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let identity = state
        .local_identities
        .active_identity()
        .await
        .ok_or(AppSessionStoreError::MissingIdentity)?;
    let response = state
        .sessions
        .revoke_session_for_identity(&session_id, &identity)
        .await?;
    Ok(Json(response))
}

pub enum AppSessionApiError {
    Store(AppSessionStoreError),
    Network(NetworkError),
    LocalIdentityCapability { identity: String },
}

impl IntoResponse for AppSessionApiError {
    fn into_response(self) -> Response {
        let err = match self {
            Self::Store(err) => err,
            Self::Network(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "code": "app_session_daemon_error",
                        "error": err.to_string()
                    })),
                )
                    .into_response();
            }
            Self::LocalIdentityCapability { identity } => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "code": "app_session_identity_not_signable",
                        "error": format!("identity {identity} cannot receive local publish, inventory, encryption, or decrypt capabilities yet")
                    })),
                )
                    .into_response();
            }
        };

        let (status, code) = match &err {
            AppSessionStoreError::MissingBearerToken | AppSessionStoreError::InvalidToken => {
                (StatusCode::UNAUTHORIZED, "app_session_unauthorized")
            }
            AppSessionStoreError::RequestNotFound(_) | AppSessionStoreError::SessionNotFound(_) => {
                (StatusCode::NOT_FOUND, "app_session_store_error")
            }
            AppSessionStoreError::DataSubscriptionNotFound(_) => {
                (StatusCode::NOT_FOUND, "data_subscription_not_found")
            }
            AppSessionStoreError::RequestNotPending(_)
            | AppSessionStoreError::MissingIdentity
            | AppSessionStoreError::CapabilityNotGrantable(_)
            | AppSessionStoreError::CapabilityNotRequested(_)
            | AppSessionStoreError::DataSubscriptionCapacityExceeded => {
                (StatusCode::BAD_REQUEST, "app_session_store_error")
            }
            AppSessionStoreError::Io(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "app_session_store_error")
            }
        };
        (
            status,
            Json(json!({
                "code": code,
                "error": err.to_string()
            })),
        )
            .into_response()
    }
}

impl From<AppSessionStoreError> for AppSessionApiError {
    fn from(err: AppSessionStoreError) -> Self {
        Self::Store(err)
    }
}

impl From<NetworkError> for AppSessionApiError {
    fn from(err: NetworkError) -> Self {
        Self::Network(err)
    }
}

fn grants_private_content_authority(capabilities: &[String]) -> bool {
    capabilities.iter().any(|capability| {
        capability.starts_with("encrypt:")
            || capability.starts_with("decrypt:")
            || capability.starts_with("publish:encrypted:")
    })
}

fn capabilities_require_daemon_identity(capabilities: &[String]) -> bool {
    capabilities.iter().any(|capability| {
        capability.starts_with("publish:")
            || capability.starts_with("inventory:")
            || capability.starts_with("pin:own:")
            || capability.starts_with("encrypt:")
            || capability.starts_with("decrypt:")
    })
}

async fn identity_can_receive_daemon_capabilities(
    state: &AppState,
    identity: &str,
) -> Result<bool, AppSessionApiError> {
    if state.local_identities.is_daemon_identity(identity).await {
        return Ok(true);
    }
    Ok(false)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AppSessionStoreError> {
    let value = headers
        .get("authorization")
        .ok_or(AppSessionStoreError::MissingBearerToken)?;
    let raw = value
        .to_str()
        .map_err(|_| AppSessionStoreError::InvalidToken)?;
    let token = raw
        .strip_prefix("Bearer ")
        .ok_or(AppSessionStoreError::InvalidToken)?;
    if token.is_empty() {
        return Err(AppSessionStoreError::InvalidToken);
    }
    Ok(token)
}
