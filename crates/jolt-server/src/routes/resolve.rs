use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use jolt_network::ResolveResponse;

#[derive(Deserialize)]
pub struct ResolveRequest {
    pub address: String,
}

pub async fn resolve_address(
    State(state): State<AppState>,
    Json(req): Json<ResolveRequest>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let result = state.daemon.resolve(req.address).await?;
    Ok(Json(result))
}
