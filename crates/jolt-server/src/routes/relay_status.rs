use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::error::ApiError;
use crate::state::AppState;
use jolt_core::RelayRecord;
use jolt_network::{CacheStatsResponse, HomeRelayConfig};

#[derive(Debug, Serialize)]
pub struct RelayStatusResponse {
    pub relay_enabled: bool,
    pub peer_id: String,
    pub identity_address: String,
    pub listen_addresses: Vec<String>,
    pub bootstrap: RelayBootstrapStatus,
    pub peers: RelayPeerStatus,
    pub known_relay_count: usize,
    pub relay_record: Option<RelayRecord>,
    pub cache: RelayCacheStatus,
    pub home_relay: Option<HomeRelayConfig>,
}

#[derive(Debug, Serialize)]
pub struct RelayBootstrapStatus {
    pub state: String,
    pub configured_relay_count: usize,
    pub effective_relay_count: usize,
    pub connected_peer_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RelayPeerStatus {
    pub connected: usize,
    pub direct: usize,
    pub relayed: usize,
}

#[derive(Debug, Serialize)]
pub struct RelayCacheStatus {
    pub cached_items: usize,
    pub cached_bytes: u64,
    pub published_items: usize,
    pub published_bytes: u64,
    pub pinned_items: usize,
    pub pinned_bytes: u64,
}

pub async fn get_status(
    State(state): State<AppState>,
) -> Result<Json<RelayStatusResponse>, ApiError> {
    let status = state.daemon.status().await?;
    let cache = state.daemon.cache_stats().await?;

    Ok(Json(RelayStatusResponse {
        relay_enabled: status.bootstrap_relay,
        peer_id: status.peer_id,
        identity_address: status.identity_address,
        listen_addresses: status.listen_addresses,
        bootstrap: RelayBootstrapStatus {
            state: status.bootstrap_state,
            configured_relay_count: status.configured_bootstrap_relay_count,
            effective_relay_count: status.effective_bootstrap_relay_count,
            connected_peer_count: status.connected_bootstrap_peers,
            last_error: status.last_bootstrap_error,
        },
        peers: RelayPeerStatus {
            connected: status.connected_peers,
            direct: status.direct_peers,
            relayed: status.relayed_peers,
        },
        known_relay_count: status.known_relay_count,
        relay_record: status.relay_record,
        cache: relay_cache_status(cache),
        home_relay: status.home_relay,
    }))
}

fn relay_cache_status(cache: CacheStatsResponse) -> RelayCacheStatus {
    RelayCacheStatus {
        cached_items: cache.cached_items,
        cached_bytes: cache.total_cached,
        published_items: cache.published_items,
        published_bytes: cache.total_published,
        pinned_items: cache.pinned_items,
        pinned_bytes: cache.pinned_size,
    }
}
