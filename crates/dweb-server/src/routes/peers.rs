use axum::extract::State;
use axum::Json;

use crate::error::ApiError;
use crate::state::AppState;
use dweb_network::PeerInfo;

pub async fn list_peers(State(state): State<AppState>) -> Result<Json<Vec<PeerInfo>>, ApiError> {
    let peers = state.daemon.peers().await?;
    Ok(Json(peers))
}
