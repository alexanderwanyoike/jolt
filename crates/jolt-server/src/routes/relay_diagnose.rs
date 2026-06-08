use std::str::FromStr;

use axum::extract::State;
use axum::Json;
use jolt_core::IdentityId;
use jolt_network::{NetworkError, RelayDiagnoseIdentityResponse};
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct DiagnoseIdentityRequest {
    pub identity: String,
}

pub async fn diagnose_identity(
    State(state): State<AppState>,
    Json(req): Json<DiagnoseIdentityRequest>,
) -> Result<Json<RelayDiagnoseIdentityResponse>, ApiError> {
    let identity = IdentityId::from_str(&req.identity)
        .map_err(|e| ApiError(NetworkError::InvalidInput(format!("invalid identity: {e}"))))?;
    let diagnosis = state.daemon.diagnose_identity(identity).await?;
    Ok(Json(diagnosis))
}
