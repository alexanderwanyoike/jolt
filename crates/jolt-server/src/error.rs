use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use jolt_network::NetworkError;

pub struct ApiError(pub NetworkError);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self.0 {
            NetworkError::ContentNotFound(id) => {
                (StatusCode::NOT_FOUND, format!("Content not found: {id}"))
            }
            NetworkError::NoPeers => (
                StatusCode::SERVICE_UNAVAILABLE,
                "No peers available".to_string(),
            ),
            NetworkError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "Request timed out".to_string()),
            NetworkError::ProviderNotFound(id) => (
                StatusCode::NOT_FOUND,
                format!("No provider found for: {id}"),
            ),
            NetworkError::InvalidInput(e) => (StatusCode::BAD_REQUEST, e.to_string()),
            NetworkError::Io(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("IO error: {e}")),
            other => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Internal error: {other}"),
            ),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<NetworkError> for ApiError {
    fn from(err: NetworkError) -> Self {
        ApiError(err)
    }
}
