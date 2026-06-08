use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use std::str::FromStr;

use crate::error::ApiError;
use crate::state::AppState;
use jolt_core::JoltAddress;
use jolt_network::DiscoveryFailureCode;
use jolt_network::FetchResult;
use jolt_network::NetworkError;

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
    let result = state
        .daemon
        .fetch(content_id.clone())
        .await
        .map_err(|err| ApiError(fetch_error_for_target(err, &content_id)))?;
    Ok(Json(result))
}

pub fn fetch_error_for_target(err: NetworkError, content_id: &str) -> NetworkError {
    match err {
        NetworkError::ProviderNotFound(id) => NetworkError::DiscoveryFailed {
            code: DiscoveryFailureCode::ContentProviderNotFound,
            message: format!("No content provider found for {id}"),
        },
        NetworkError::Timeout => NetworkError::DiscoveryFailed {
            code: DiscoveryFailureCode::ContentFetchFailed,
            message: format!("Timed out while fetching content {content_id}"),
        },
        NetworkError::VerificationFailed => NetworkError::DiscoveryFailed {
            code: DiscoveryFailureCode::ContentHashMismatch,
            message: format!("Fetched content did not match requested hash {content_id}"),
        },
        other => other,
    }
}
