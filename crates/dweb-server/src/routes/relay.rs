use axum::extract::State;
use axum::Json;
use dweb_core::PinRequest;
use dweb_network::NetworkError;
use serde::Serialize;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct RelayPinResponse {
    pub status: String,
    pub owner: String,
    pub content_id: String,
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
    let owner = request
        .owner_identity()
        .map_err(|e| {
            ApiError(NetworkError::InvalidInput(format!(
                "invalid pin request: {e}"
            )))
        })?
        .to_string();
    let content_id = request.body.content_id.to_string();

    let fetched = state.daemon.fetch(content_id.clone()).await?;
    state.daemon.pin(content_id.clone()).await?;

    Ok(Json(RelayPinResponse {
        status: "pinned".to_string(),
        owner,
        content_id,
        size: fetched.size,
    }))
}
