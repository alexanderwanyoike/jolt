use axum::extract::State;
use axum::Json;

use crate::error::ApiError;
use crate::state::AppState;
use dweb_network::NodeStatus;

pub async fn get_status(State(state): State<AppState>) -> Result<Json<NodeStatus>, ApiError> {
    let status = state.daemon.status().await?;
    Ok(Json(status))
}
