use axum::extract::State;
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
