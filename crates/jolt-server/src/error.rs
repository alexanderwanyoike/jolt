use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use jolt_network::NetworkError;

pub struct ApiError(pub NetworkError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self.0 {
            NetworkError::ContentNotFound(id) => (
                StatusCode::NOT_FOUND,
                "content_not_found",
                format!("Content not found: {id}"),
            ),
            NetworkError::NoPeers => (
                StatusCode::SERVICE_UNAVAILABLE,
                "no_peers",
                "No peers available".to_string(),
            ),
            NetworkError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "timeout",
                "Request timed out".to_string(),
            ),
            NetworkError::ProviderNotFound(id) => (
                StatusCode::NOT_FOUND,
                "provider_not_found",
                format!("No provider found for: {id}"),
            ),
            NetworkError::DiscoveryFailed { code, message } => {
                (StatusCode::NOT_FOUND, code.as_str(), message.clone())
            }
            NetworkError::InvalidInput(e) => {
                (StatusCode::BAD_REQUEST, "invalid_input", e.to_string())
            }
            NetworkError::RecordConflict => (
                StatusCode::CONFLICT,
                "record_conflict",
                "Record changed since it was read".to_string(),
            ),
            NetworkError::PathTombstoned { path } => (
                StatusCode::GONE,
                "path_tombstoned",
                format!("Path is tombstoned: {path}"),
            ),
            NetworkError::VerificationFailed => (
                StatusCode::BAD_GATEWAY,
                "content_hash_mismatch",
                "Verification failed: hash mismatch".to_string(),
            ),
            NetworkError::Io(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "io_error",
                format!("IO error: {e}"),
            ),
            other => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                format!("Internal error: {other}"),
            ),
        };

        (status, Json(json!({ "code": code, "error": message }))).into_response()
    }
}

impl From<NetworkError> for ApiError {
    fn from(err: NetworkError) -> Self {
        ApiError(err)
    }
}
