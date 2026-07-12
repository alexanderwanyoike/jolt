use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::local_identities::{
    CreateLocalIdentityRequest, LocalIdentitiesResponse, LocalIdentityError, LocalIdentityView,
    SelectLocalIdentityRequest,
};
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<LocalIdentitiesResponse>, LocalIdentityApiError> {
    Ok(Json(state.local_identities.list().await))
}

pub async fn create(
    State(state): State<AppState>,
    Json(request): Json<CreateLocalIdentityRequest>,
) -> Result<Json<LocalIdentityView>, LocalIdentityApiError> {
    Ok(Json(state.local_identities.create(request).await?))
}

pub async fn select_active(
    State(state): State<AppState>,
    Json(request): Json<SelectLocalIdentityRequest>,
) -> Result<Json<LocalIdentitiesResponse>, LocalIdentityApiError> {
    Ok(Json(state.local_identities.select(request).await?))
}

pub async fn delete_identity(
    State(state): State<AppState>,
    Path(identity): Path<String>,
) -> Result<Json<LocalIdentitiesResponse>, LocalIdentityApiError> {
    Ok(Json(state.local_identities.delete(identity).await?))
}

pub struct LocalIdentityApiError(LocalIdentityError);

impl IntoResponse for LocalIdentityApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            LocalIdentityError::MissingIdentity => {
                (StatusCode::BAD_REQUEST, "local_identity_missing")
            }
            LocalIdentityError::UnknownIdentity(_) => {
                (StatusCode::NOT_FOUND, "local_identity_not_found")
            }
            LocalIdentityError::ProtectedIdentity(_) => {
                (StatusCode::BAD_REQUEST, "local_identity_protected")
            }
            LocalIdentityError::Storage(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "local_identity_storage")
            }
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

impl From<LocalIdentityError> for LocalIdentityApiError {
    fn from(error: LocalIdentityError) -> Self {
        Self(error)
    }
}
