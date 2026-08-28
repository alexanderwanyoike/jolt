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
            NetworkError::LocalDeviceRevoked { device_id } => (
                StatusCode::FORBIDDEN,
                "device_revoked",
                format!("Local device is revoked: {device_id}"),
            ),
            NetworkError::LocalDeviceSigningKeyMismatch { device_id } => (
                StatusCode::FORBIDDEN,
                "device_signing_key_mismatch",
                format!(
                    "Local device signing key does not match its authority record: {device_id}"
                ),
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

#[cfg(test)]
mod tests {
    use axum::body::to_bytes;

    use super::*;

    #[tokio::test]
    async fn local_device_signing_key_mismatch_is_a_structured_forbidden_response() {
        let response = ApiError(NetworkError::LocalDeviceSigningKeyMismatch {
            device_id: "dev_local".to_string(),
        })
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["code"], "device_signing_key_mismatch");
        assert!(body["error"].as_str().unwrap().contains("dev_local"));
    }
}
