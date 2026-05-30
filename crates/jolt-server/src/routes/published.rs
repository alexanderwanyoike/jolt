use axum::extract::State;
use axum::Json;
use jolt_network::PublishedContentInfo;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<PublishedContentInfo>>, ApiError> {
    let entries = state.daemon.list_published_content().await?;
    Ok(Json(entries))
}
