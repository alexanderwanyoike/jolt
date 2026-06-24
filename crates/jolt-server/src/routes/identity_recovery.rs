use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::identity_recovery::{
    ExportIdentityRequest, IdentityRecoveryError, ImportIdentityRequest, ImportIdentityResponse,
};
use crate::state::AppState;

pub async fn export_identity(
    State(state): State<AppState>,
    Json(request): Json<ExportIdentityRequest>,
) -> Result<impl IntoResponse, IdentityRecoveryApiError> {
    Ok(Json(state.identity_recovery.export_identity(request)?))
}

pub async fn import_identity(
    State(state): State<AppState>,
    Json(request): Json<ImportIdentityRequest>,
) -> Result<impl IntoResponse, IdentityRecoveryApiError> {
    if request.as_local_identity {
        let imported = state.identity_recovery.import_local_identity(request)?;
        let encryption_key_count = imported.encryption_key_count;
        let imported_view = state
            .local_identities
            .import_recovered(imported.identity, imported.label)
            .await;
        return Ok(Json(ImportIdentityResponse {
            identity: imported_view.address,
            imported: true,
            restart_required: false,
            encryption_key_count,
            app_sessions_imported: false,
        }));
    }
    Ok(Json(state.identity_recovery.import_identity(request)?))
}

pub struct IdentityRecoveryApiError(IdentityRecoveryError);

impl IntoResponse for IdentityRecoveryApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            IdentityRecoveryError::Bundle(_)
            | IdentityRecoveryError::Identity(_)
            | IdentityRecoveryError::Store(_) => {
                (StatusCode::BAD_REQUEST, "identity_recovery_invalid")
            }
            IdentityRecoveryError::WouldOverwrite { .. } => {
                (StatusCode::CONFLICT, "identity_recovery_would_overwrite")
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

impl From<IdentityRecoveryError> for IdentityRecoveryApiError {
    fn from(err: IdentityRecoveryError) -> Self {
        Self(err)
    }
}
