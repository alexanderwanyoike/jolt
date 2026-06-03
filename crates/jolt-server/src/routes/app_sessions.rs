use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::session_store::{AppSessionRequest, AppSessionStoreError, ApproveAppSessionRequest};
use crate::state::AppState;

pub async fn request_session(
    State(state): State<AppState>,
    Json(request): Json<AppSessionRequest>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.create_request(request).await?;
    Ok(Json(response))
}

pub async fn get_request_status(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.request_status(&request_id).await?;
    Ok(Json(response))
}

pub async fn current_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let token = bearer_token(&headers)?.to_string();
    let response = state.sessions.session_for_token(&token).await?;
    Ok(Json(response))
}

pub async fn list_requests(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    Ok(Json(state.sessions.list_requests().await))
}

pub async fn approve_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(request): Json<ApproveAppSessionRequest>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.approve_request(&request_id, request).await?;
    Ok(Json(response))
}

pub async fn reject_request(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.reject_request(&request_id).await?;
    Ok(Json(response))
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    Ok(Json(state.sessions.list_sessions().await))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppSessionApiError> {
    let response = state.sessions.revoke_session(&session_id).await?;
    Ok(Json(response))
}

pub struct AppSessionApiError(AppSessionStoreError);

impl IntoResponse for AppSessionApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self.0 {
            AppSessionStoreError::MissingBearerToken | AppSessionStoreError::InvalidToken => {
                (StatusCode::UNAUTHORIZED, "app_session_unauthorized")
            }
            AppSessionStoreError::RequestNotFound(_) | AppSessionStoreError::SessionNotFound(_) => {
                (StatusCode::NOT_FOUND, "app_session_store_error")
            }
            AppSessionStoreError::RequestNotPending(_) | AppSessionStoreError::MissingIdentity => {
                (StatusCode::BAD_REQUEST, "app_session_store_error")
            }
            AppSessionStoreError::Io(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "app_session_store_error")
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

impl From<AppSessionStoreError> for AppSessionApiError {
    fn from(err: AppSessionStoreError) -> Self {
        Self(err)
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AppSessionStoreError> {
    let value = headers
        .get("authorization")
        .ok_or(AppSessionStoreError::MissingBearerToken)?;
    let raw = value
        .to_str()
        .map_err(|_| AppSessionStoreError::InvalidToken)?;
    let token = raw
        .strip_prefix("Bearer ")
        .ok_or(AppSessionStoreError::InvalidToken)?;
    if token.is_empty() {
        return Err(AppSessionStoreError::InvalidToken);
    }
    Ok(token)
}
