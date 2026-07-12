use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::identity_recovery::{
    ExportIdentityRequest, IdentityRecoveryError, ImportIdentityRequest, ImportIdentityResponse,
};
use crate::local_identities::LocalIdentityError;
use crate::state::AppState;

pub async fn export_identity(
    State(state): State<AppState>,
    Json(request): Json<ExportIdentityRequest>,
) -> Result<impl IntoResponse, IdentityRecoveryApiError> {
    if let Some(requested_identity) = request.identity.as_deref() {
        if let Some(identity) = state
            .local_identities
            .generated_identity(requested_identity)
            .await?
        {
            let encryption_keypairs = state
                .local_identities
                .generated_identity_encryption_keypairs(requested_identity)
                .await?;
            return Ok(Json(state.identity_recovery.export_local_identity(
                identity,
                encryption_keypairs,
                request,
            )?));
        }
    }

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
            .import_recovered(
                imported.identity,
                imported.label,
                imported.encryption_keypairs,
            )
            .await?;
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

pub enum IdentityRecoveryApiError {
    Recovery(IdentityRecoveryError),
    LocalIdentity(LocalIdentityError),
}

impl IntoResponse for IdentityRecoveryApiError {
    fn into_response(self) -> Response {
        let (status, code, error) = match self {
            IdentityRecoveryApiError::Recovery(error) => {
                let (status, code) = match &error {
                    IdentityRecoveryError::Bundle(_)
                    | IdentityRecoveryError::Identity(_)
                    | IdentityRecoveryError::Store(_) => {
                        (StatusCode::BAD_REQUEST, "identity_recovery_invalid")
                    }
                    IdentityRecoveryError::WouldOverwrite { .. } => {
                        (StatusCode::CONFLICT, "identity_recovery_would_overwrite")
                    }
                };
                (status, code, error.to_string())
            }
            IdentityRecoveryApiError::LocalIdentity(error) => {
                let (status, code) = match &error {
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
                (status, code, error.to_string())
            }
        };
        (
            status,
            Json(json!({
                "code": code,
                "error": error
            })),
        )
            .into_response()
    }
}

impl From<IdentityRecoveryError> for IdentityRecoveryApiError {
    fn from(err: IdentityRecoveryError) -> Self {
        Self::Recovery(err)
    }
}

impl From<LocalIdentityError> for IdentityRecoveryApiError {
    fn from(error: LocalIdentityError) -> Self {
        Self::LocalIdentity(error)
    }
}
