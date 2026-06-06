use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use jolt_network::{DecryptedObjectResponse, IngressRecord};
use serde::Deserialize;

use crate::error::ApiError;
use crate::routes::app_api::{
    authenticated_session, require_capability, require_local_identity, AppApiError,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SubmitIngressRequest {
    pub receiver_id: String,
    pub encrypted_object: Vec<u8>,
    pub expires_at: Option<u64>,
}

pub async fn submit_ingress(
    State(state): State<AppState>,
    Json(req): Json<SubmitIngressRequest>,
) -> Result<Json<IngressRecord>, ApiError> {
    let record = state
        .daemon
        .submit_ingress(req.receiver_id, req.encrypted_object, req.expires_at)
        .await?;
    Ok(Json(record))
}

pub async fn list_pending_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<IngressRecord>>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:read")?;
    let records = state.daemon.list_pending_ingress().await?;
    Ok(Json(records))
}

pub async fn open_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<DecryptedObjectResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:read")?;
    let record = state.daemon.open_ingress(ingress_id).await?;
    Ok(Json(record))
}

pub async fn accept_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<IngressRecord>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:decide")?;
    let record = state.daemon.accept_ingress(ingress_id).await?;
    Ok(Json(record))
}

pub async fn reject_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<IngressRecord>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:decide")?;
    let record = state.daemon.reject_ingress(ingress_id).await?;
    Ok(Json(record))
}
