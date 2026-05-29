use axum::extract::State;
use axum::Json;
use dweb_network::{HomeRelayCapability, HomeRelayConfig, NetworkError};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration, Instant};

use crate::error::ApiError;
use crate::routes::relay::RelayPinResponse;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct HomeRelayPinRequest {
    pub content_id: String,
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HomeRelayPinResponse {
    pub status: String,
    pub relay: String,
    pub owner: String,
    pub content_id: String,
    pub latest_sequence: u64,
    pub size: u64,
}

pub async fn pin(
    State(state): State<AppState>,
    Json(request): Json<HomeRelayPinRequest>,
) -> Result<Json<HomeRelayPinResponse>, ApiError> {
    let status = state.daemon.status().await?;
    let relay = status.home_relay.ok_or_else(|| {
        ApiError(NetworkError::InvalidInput(
            "home relay is not configured".to_string(),
        ))
    })?;
    if relay.capability != HomeRelayCapability::Pinning {
        return Err(ApiError(NetworkError::InvalidInput(
            "home relay is not pin-capable".to_string(),
        )));
    }
    let api_url = relay.api_url.clone().ok_or_else(|| {
        ApiError(NetworkError::InvalidInput(
            "home relay API URL is not configured".to_string(),
        ))
    })?;

    ensure_connected_to_relay(&state, &relay).await?;

    let pin_request = state
        .daemon
        .create_pin_request(request.content_id.clone())
        .await?;
    let relay_response = reqwest::Client::new()
        .post(format!(
            "{}/api/v1/relay/pins",
            api_url.trim_end_matches('/')
        ))
        .json(&pin_request)
        .send()
        .await
        .map_err(|e| {
            ApiError(NetworkError::Protocol(format!(
                "home relay request failed: {e}"
            )))
        })?;

    let relay_status = relay_response.status();
    if !relay_status.is_success() {
        let body = relay_response.text().await.unwrap_or_default();
        return Err(ApiError(NetworkError::InvalidInput(format!(
            "home relay rejected pin request ({relay_status}): {body}"
        ))));
    }

    let pinned = relay_response
        .json::<RelayPinResponse>()
        .await
        .map_err(|e| {
            ApiError(NetworkError::Protocol(format!(
                "home relay returned invalid pin response: {e}"
            )))
        })?;

    state
        .daemon
        .record_home_relay_pin(
            request.content_id,
            request.path,
            relay.clone(),
            pinned.latest_sequence,
        )
        .await?;

    Ok(Json(HomeRelayPinResponse {
        status: pinned.status,
        relay: relay.peer_id,
        owner: pinned.owner,
        content_id: pinned.content_id,
        latest_sequence: pinned.latest_sequence,
        size: pinned.size,
    }))
}

async fn ensure_connected_to_relay(
    state: &AppState,
    relay: &HomeRelayConfig,
) -> Result<(), ApiError> {
    if is_connected_to_relay(state, relay).await? {
        return Ok(());
    }

    state.daemon.connect_peer(relay.multiaddr.clone()).await?;

    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if is_connected_to_relay(state, relay).await? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ApiError(NetworkError::Timeout));
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn is_connected_to_relay(
    state: &AppState,
    relay: &HomeRelayConfig,
) -> Result<bool, ApiError> {
    let peers = state.daemon.peers().await?;
    Ok(peers.iter().any(|peer| peer.peer_id == relay.peer_id))
}
