use axum::extract::{Path, State};
use axum::Json;
use jolt_core::{PinRequest, UpdateLogEntry};
use jolt_network::NetworkError;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize, Serialize)]
pub struct RelayPinResponse {
    pub status: String,
    pub owner: String,
    pub content_id: String,
    pub latest_sequence: u64,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelayPinStatusResponse {
    pub content_id: String,
    pub pinned: bool,
    pub size: Option<u64>,
}

pub async fn create_pin(
    State(state): State<AppState>,
    Json(request): Json<PinRequest>,
) -> Result<Json<RelayPinResponse>, ApiError> {
    let status = state.daemon.status().await?;
    if !status.bootstrap_relay {
        return Err(ApiError(NetworkError::InvalidInput(
            "node is not configured to accept relay pin requests".to_string(),
        )));
    }

    request.verify().map_err(|e| {
        ApiError(NetworkError::InvalidInput(format!(
            "invalid pin request: {e}"
        )))
    })?;
    let owner_identity = request.owner_identity().map_err(|e| {
        ApiError(NetworkError::InvalidInput(format!(
            "invalid pin request: {e}"
        )))
    })?;
    let owner = owner_identity.to_string();
    if !state.daemon.relay_pin_allowed(owner.clone()).await? {
        return Err(ApiError(NetworkError::InvalidInput(
            "relay pin denied: identity is not allowlisted".to_string(),
        )));
    }
    let content_id = request.body.content_id.to_string();

    let latest_sequence = if let Some(update_log_content_id) = request.body.update_log_content_id {
        let fetched_log = state
            .daemon
            .fetch(update_log_content_id.to_string())
            .await?;
        let entries: Vec<UpdateLogEntry> =
            serde_json::from_slice(&fetched_log.data).map_err(|e| {
                ApiError(NetworkError::Protocol(format!(
                    "invalid update log snapshot: {e}"
                )))
            })?;
        let latest_sequence = state
            .daemon
            .store_update_log(owner_identity.clone(), entries)
            .await?;
        state.daemon.pin(update_log_content_id.to_string()).await?;
        latest_sequence
    } else {
        state.daemon.pin_update_log(owner_identity).await?
    };
    let fetched = state.daemon.fetch(content_id.clone()).await?;
    state.daemon.pin(content_id.clone()).await?;

    Ok(Json(RelayPinResponse {
        status: "pinned".to_string(),
        owner,
        content_id,
        latest_sequence,
        size: fetched.size,
    }))
}

pub async fn pin_status(
    State(state): State<AppState>,
    Path(content_id): Path<String>,
) -> Result<Json<RelayPinStatusResponse>, ApiError> {
    let status = state.daemon.status().await?;
    if !status.bootstrap_relay {
        return Err(ApiError(NetworkError::InvalidInput(
            "node is not configured to report relay pin status".to_string(),
        )));
    }

    let entry = state
        .daemon
        .list_cache_entries()
        .await?
        .into_iter()
        .find(|entry| entry.content_id == content_id);
    Ok(Json(RelayPinStatusResponse {
        content_id,
        pinned: entry.as_ref().map(|entry| entry.pinned).unwrap_or(false),
        size: entry.map(|entry| entry.size),
    }))
}
