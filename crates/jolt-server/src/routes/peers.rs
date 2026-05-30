use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use jolt_network::{PeerConnectResponse, PeerInfo};

#[derive(Deserialize)]
pub struct ConnectPeerRequest {
    pub multiaddr: String,
}

pub async fn list_peers(State(state): State<AppState>) -> Result<Json<Vec<PeerInfo>>, ApiError> {
    let peers = state.daemon.peers().await?;
    Ok(Json(peers))
}

pub async fn connect_peer(
    State(state): State<AppState>,
    Json(req): Json<ConnectPeerRequest>,
) -> Result<Json<PeerConnectResponse>, ApiError> {
    let result = state.daemon.connect_peer(req.multiaddr).await?;
    Ok(Json(result))
}
