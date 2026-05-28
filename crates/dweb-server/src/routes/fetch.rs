use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use std::str::FromStr;

use crate::error::ApiError;
use crate::state::AppState;
use dweb_core::JoltAddress;
use dweb_network::FetchResult;
use dweb_network::NetworkError;

#[derive(Deserialize)]
pub struct FetchRequest {
    pub content_id: Option<String>,
    pub target: Option<String>,
}

pub async fn fetch_content(
    State(state): State<AppState>,
    Json(req): Json<FetchRequest>,
) -> Result<Json<FetchResult>, ApiError> {
    let target = req.target.or(req.content_id).ok_or_else(|| {
        ApiError(NetworkError::InvalidInput(
            "missing fetch target".to_string(),
        ))
    })?;
    let content_id = match JoltAddress::from_str(&target) {
        Ok(_) => state.daemon.resolve(target).await?.content_id,
        Err(e) if target.contains(".jolt") => {
            return Err(ApiError(NetworkError::InvalidInput(e.to_string())));
        }
        Err(_) => target,
    };
    let result = state.daemon.fetch(content_id).await?;
    Ok(Json(result))
}
