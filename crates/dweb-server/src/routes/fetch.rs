use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::error::ApiError;
use crate::state::AppState;
use dweb_network::FetchResult;

#[derive(Deserialize)]
pub struct FetchRequest {
    pub content_id: String,
}

pub async fn fetch_content(
    State(state): State<AppState>,
    Json(req): Json<FetchRequest>,
) -> Result<Json<FetchResult>, ApiError> {
    let result = state.daemon.fetch(req.content_id).await?;
    Ok(Json(result))
}
