use axum::extract::State;
use axum::Json;
use jolt_network::{HomeRelayCapability, HomeRelayConfig, NetworkError, PublishedRelayInfo};
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration, Instant};

use crate::error::ApiError;
use crate::routes::relay::{RelayPinResponse, RelayPinStatusResponse};
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

#[derive(Debug, Serialize)]
pub struct HomeRelayAvailabilityResponse {
    pub status: String,
    pub checked_count: usize,
    pub degraded_count: usize,
    pub items: Vec<HomeRelayAvailabilityItem>,
}

#[derive(Debug, Serialize)]
pub struct HomeRelayAvailabilityItem {
    pub content_id: String,
    pub path: Option<String>,
    pub address: Option<String>,
    pub relay: PublishedRelayInfo,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

pub async fn availability(
    State(state): State<AppState>,
) -> Result<Json<HomeRelayAvailabilityResponse>, ApiError> {
    let pinned_items = state
        .daemon
        .list_published_content()
        .await?
        .into_iter()
        .filter_map(|item| {
            let relay = item.relay?;
            let content_id = item.pinned_content_id?;
            Some((item.path, item.address, relay, content_id))
        })
        .collect::<Vec<_>>();

    let client = reqwest::Client::new();
    let mut items = Vec::with_capacity(pinned_items.len());
    for (path, address, relay, content_id) in pinned_items {
        let (status, error) = check_relay_pin(&client, &relay, &content_id).await;
        items.push(HomeRelayAvailabilityItem {
            content_id,
            path,
            address,
            relay,
            status,
            error,
        });
    }

    let degraded_count = items
        .iter()
        .filter(|item| item.status != "available")
        .count();
    let status = if items.is_empty() {
        "unknown"
    } else if degraded_count == 0 {
        "healthy"
    } else {
        "degraded"
    };

    Ok(Json(HomeRelayAvailabilityResponse {
        status: status.to_string(),
        checked_count: items.len(),
        degraded_count,
        items,
    }))
}

async fn check_relay_pin(
    client: &reqwest::Client,
    relay: &PublishedRelayInfo,
    content_id: &str,
) -> (String, Option<String>) {
    let Some(api_url) = relay.api_url.as_deref() else {
        return (
            "relay_unreachable".to_string(),
            Some("home relay API URL is not configured".to_string()),
        );
    };
    let url = format!(
        "{}/api/v1/relay/pins/{content_id}",
        api_url.trim_end_matches('/')
    );
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(e) => {
            return (
                "relay_unreachable".to_string(),
                Some(format!("home relay request failed: {e}")),
            );
        }
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return (
            "relay_unreachable".to_string(),
            Some(format!(
                "home relay status request failed ({status}): {body}"
            )),
        );
    }
    match response.json::<RelayPinStatusResponse>().await {
        Ok(pin) if pin.pinned => ("available".to_string(), None),
        Ok(_) => ("missing_pin".to_string(), None),
        Err(e) => (
            "relay_unreachable".to_string(),
            Some(format!("home relay returned invalid pin status: {e}")),
        ),
    }
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
