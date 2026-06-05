use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use jolt_network::{HomeRelayCapability, NetworkError};
use serde::Deserialize;
use serde_json::json;

use crate::network_settings::{NetworkSettingsError, NetworkSettingsResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct BootstrapRelayRequest {
    pub multiaddr: String,
}

#[derive(Debug, Deserialize)]
pub struct HomeRelayRequest {
    pub multiaddr: String,
    pub capability: HomeRelayCapability,
    pub api_url: Option<String>,
}

pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<NetworkSettingsResponse>, NetworkSettingsApiError> {
    Ok(Json(state.network_settings.response()?))
}

pub async fn add_bootstrap_relay(
    State(state): State<AppState>,
    Json(request): Json<BootstrapRelayRequest>,
) -> Result<Json<NetworkSettingsResponse>, NetworkSettingsApiError> {
    let response = state
        .network_settings
        .add_bootstrap_relay(&request.multiaddr)?;
    apply_runtime_settings(&state, &response).await?;
    Ok(Json(response))
}

pub async fn remove_bootstrap_relay(
    State(state): State<AppState>,
    Json(request): Json<BootstrapRelayRequest>,
) -> Result<Json<NetworkSettingsResponse>, NetworkSettingsApiError> {
    let response = state
        .network_settings
        .remove_bootstrap_relay(&request.multiaddr)?;
    apply_runtime_settings(&state, &response).await?;
    Ok(Json(response))
}

pub async fn set_home_relay(
    State(state): State<AppState>,
    Json(request): Json<HomeRelayRequest>,
) -> Result<Json<NetworkSettingsResponse>, NetworkSettingsApiError> {
    let response = state.network_settings.set_home_relay(
        &request.multiaddr,
        request.capability,
        request.api_url.as_deref(),
    )?;
    apply_runtime_settings(&state, &response).await?;
    Ok(Json(response))
}

pub async fn clear_home_relay(
    State(state): State<AppState>,
) -> Result<Json<NetworkSettingsResponse>, NetworkSettingsApiError> {
    let response = state.network_settings.clear_home_relay()?;
    apply_runtime_settings(&state, &response).await?;
    Ok(Json(response))
}

async fn apply_runtime_settings(
    state: &AppState,
    response: &NetworkSettingsResponse,
) -> Result<(), NetworkSettingsApiError> {
    state
        .daemon
        .update_network_settings(
            response.configured_bootstrap_relays.clone(),
            response.effective_bootstrap_relays.clone(),
            response.home_relay.clone(),
        )
        .await
        .map_err(NetworkSettingsApiError::Daemon)
}

pub enum NetworkSettingsApiError {
    Settings(NetworkSettingsError),
    Daemon(NetworkError),
}

impl IntoResponse for NetworkSettingsApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Settings(NetworkSettingsError::Invalid(message)) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "code": "invalid_network_settings", "error": message })),
            )
                .into_response(),
            Self::Settings(NetworkSettingsError::Storage(message)) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "network_settings_storage", "error": message })),
            )
                .into_response(),
            Self::Daemon(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "code": "network_settings_runtime", "error": error.to_string() })),
            )
                .into_response(),
        }
    }
}

impl From<NetworkSettingsError> for NetworkSettingsApiError {
    fn from(error: NetworkSettingsError) -> Self {
        Self::Settings(error)
    }
}
