use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::device_authority::{AuthorizeDeviceRequest, DeviceAuthorityError, RevokeDeviceRequest};
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, DeviceAuthorityApiError> {
    Ok(Json(state.device_authority.list(&state.daemon).await?))
}

pub async fn authorize_device(
    State(state): State<AppState>,
    Json(request): Json<AuthorizeDeviceRequest>,
) -> Result<impl IntoResponse, DeviceAuthorityApiError> {
    Ok(Json(
        state
            .device_authority
            .authorize_device(&state.daemon, request)
            .await?,
    ))
}

pub async fn revoke_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    Json(request): Json<RevokeDeviceRequest>,
) -> Result<impl IntoResponse, DeviceAuthorityApiError> {
    Ok(Json(
        state
            .device_authority
            .revoke_device(&state.daemon, device_id, request)
            .await?,
    ))
}

pub struct DeviceAuthorityApiError(DeviceAuthorityError);

impl IntoResponse for DeviceAuthorityApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            DeviceAuthorityError::UnknownDevice(_) => {
                (StatusCode::NOT_FOUND, "device_authority_unknown_device")
            }
            DeviceAuthorityError::MissingLocalIdentity
            | DeviceAuthorityError::InvalidLocalIdentity(_)
            | DeviceAuthorityError::Authority(_) => {
                (StatusCode::BAD_REQUEST, "device_authority_invalid")
            }
            DeviceAuthorityError::Network(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "device_authority_daemon_error",
            ),
        };

        (
            status,
            Json(json!({
                "code": code,
                "error": self.0.to_string()
            })),
        )
            .into_response()
    }
}

impl From<DeviceAuthorityError> for DeviceAuthorityApiError {
    fn from(err: DeviceAuthorityError) -> Self {
        Self(err)
    }
}
