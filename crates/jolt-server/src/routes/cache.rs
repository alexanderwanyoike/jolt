use axum::extract::{Path, State};
use axum::Json;

use crate::error::ApiError;
use crate::state::AppState;
use jolt_network::{CacheEntryInfo, CacheStatsResponse};

pub async fn stats(State(state): State<AppState>) -> Result<Json<CacheStatsResponse>, ApiError> {
    let stats = state.daemon.cache_stats().await?;
    Ok(Json(stats))
}

pub async fn list_entries(
    State(state): State<AppState>,
) -> Result<Json<Vec<CacheEntryInfo>>, ApiError> {
    let entries = state.daemon.list_cache_entries().await?;
    Ok(Json(entries))
}

pub async fn pin(
    State(state): State<AppState>,
    Path(content_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.daemon.pin(content_id).await?;
    Ok(Json(serde_json::json!({ "status": "pinned" })))
}

pub async fn unpin(
    State(state): State<AppState>,
    Path(content_id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    state.daemon.unpin(content_id).await?;
    Ok(Json(serde_json::json!({ "status": "unpinned" })))
}
