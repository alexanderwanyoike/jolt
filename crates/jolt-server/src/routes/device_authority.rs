use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::device_authority::{AuthorizeDeviceRequest, DeviceAuthorityError, RevokeDeviceRequest};
use crate::session_store::AppSessionStoreError;
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
    let response = state
        .device_authority
        .revoke_device(&state.daemon, device_id.clone(), request)
        .await?;
    state
        .sessions
        .revoke_sessions_for_device(&device_id)
        .await?;
    Ok(Json(response))
}

pub enum DeviceAuthorityApiError {
    Authority(DeviceAuthorityError),
    Sessions(AppSessionStoreError),
}

impl IntoResponse for DeviceAuthorityApiError {
    fn into_response(self) -> Response {
        let err = match self {
            Self::Authority(err) => err,
            Self::Sessions(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "code": "device_authority_session_revoke_failed",
                        "error": err.to_string()
                    })),
                )
                    .into_response();
            }
        };

        let (status, code) = match &err {
            DeviceAuthorityError::UnknownDevice(_) => {
                (StatusCode::NOT_FOUND, "device_authority_unknown_device")
            }
            DeviceAuthorityError::InvalidEnrollment(_) => {
                (StatusCode::BAD_REQUEST, "device_enrollment_invalid")
            }
            DeviceAuthorityError::MissingLocalIdentity
            | DeviceAuthorityError::InvalidLocalIdentity(_)
            | DeviceAuthorityError::Authority(_) => {
                (StatusCode::BAD_REQUEST, "device_authority_invalid")
            }
            DeviceAuthorityError::Network(jolt_network::NetworkError::InvalidInput(_)) => {
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
                "error": err.to_string()
            })),
        )
            .into_response()
    }
}

impl From<DeviceAuthorityError> for DeviceAuthorityApiError {
    fn from(err: DeviceAuthorityError) -> Self {
        Self::Authority(err)
    }
}

impl From<AppSessionStoreError> for DeviceAuthorityApiError {
    fn from(err: AppSessionStoreError) -> Self {
        Self::Sessions(err)
    }
}
