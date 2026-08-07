use axum::extract::{Path, State};
use axum::Json;
use jolt_core::{PinRequest, UpdateLogEntry};
use jolt_network::{NetworkError, RelayPinItem, RelayPinRequestItems};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize, Serialize)]
pub struct RelayPinResponse {
    pub status: String,
    pub owner: String,
    pub content_id: String,
    pub latest_sequence: u64,
    pub size: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelayPinStatusResponse {
    pub content_id: String,
    pub pinned: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RelayCapabilitiesResponse {
    pub pin_request_versions: Vec<String>,
}

pub async fn capabilities() -> Json<RelayCapabilitiesResponse> {
    Json(RelayCapabilitiesResponse {
        pin_request_versions: vec!["v1".to_string(), "v1-signed-sizes".to_string()],
    })
}

pub async fn create_pin(
    State(state): State<AppState>,
    Json(request): Json<PinRequest>,
) -> Result<Json<RelayPinResponse>, ApiError> {
    let status = state.daemon.status().await?;
    if !status.bootstrap_relay {
        return Err(ApiError(NetworkError::InvalidInput(
            "node is not configured to accept relay pin requests".to_string(),
        )));
    }

    request.verify().map_err(|e| {
        ApiError(NetworkError::InvalidInput(format!(
            "invalid pin request: {e}"
        )))
    })?;
    let owner_identity = request.owner_identity().map_err(|e| {
        ApiError(NetworkError::InvalidInput(format!(
            "invalid pin request: {e}"
        )))
    })?;
    let owner = owner_identity.to_string();
    let content_id = request.body.content_id.to_string();

    let requested_items = requested_pin_items(&request)?;
    let reservation_id = state
        .daemon
        .reserve_relay_pin(owner.clone(), requested_items)
        .await?;

    let prepared = async {
        let update_log =
            if let Some(update_log_content_id) = request.body.update_log_content_id.clone() {
                let fetched = state
                    .daemon
                    .fetch(update_log_content_id.to_string())
                    .await?;
                let entries: Vec<UpdateLogEntry> =
                    serde_json::from_slice(&fetched.data).map_err(|e| {
                        ApiError(NetworkError::Protocol(format!(
                            "invalid update log snapshot: {e}"
                        )))
                    })?;
                Some((update_log_content_id, fetched.size, entries))
            } else {
                None
            };
        let fetched = state.daemon.fetch(content_id.clone()).await?;
        let mut actual_items = vec![RelayPinItem {
            content_id: content_id.clone(),
            size: fetched.size,
        }];
        if let Some((update_log_content_id, size, _)) = &update_log {
            actual_items.push(RelayPinItem {
                content_id: update_log_content_id.to_string(),
                size: *size,
            });
        }
        Ok::<_, ApiError>((update_log, fetched, actual_items))
    }
    .await;

    let (update_log, fetched, actual_items) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = state.daemon.cancel_relay_pin(reservation_id).await;
            return Err(error);
        }
    };
    if let Err(error) = state
        .daemon
        .validate_relay_pin(reservation_id, actual_items.clone())
        .await
    {
        let _ = state.daemon.cancel_relay_pin(reservation_id).await;
        return Err(ApiError(error));
    }

    let finalized = async {
        let latest_sequence = if let Some((update_log_content_id, _, entries)) = update_log {
            let latest_sequence = state
                .daemon
                .store_update_log(owner_identity.clone(), entries)
                .await?;
            state.daemon.pin(update_log_content_id.to_string()).await?;
            latest_sequence
        } else {
            state.daemon.pin_update_log(owner_identity).await?
        };
        state.daemon.pin(content_id.clone()).await?;
        state
            .daemon
            .commit_relay_pin(reservation_id, actual_items)
            .await?;
        Ok::<_, NetworkError>(latest_sequence)
    }
    .await;
    let latest_sequence = match finalized {
        Ok(sequence) => sequence,
        Err(error) => {
            let _ = state.daemon.cancel_relay_pin(reservation_id).await;
            return Err(ApiError(error));
        }
    };

    Ok(Json(RelayPinResponse {
        status: "pinned".to_string(),
        owner,
        content_id,
        latest_sequence,
        size: fetched.size,
    }))
}

fn requested_pin_items(request: &PinRequest) -> Result<RelayPinRequestItems, ApiError> {
    match (
        request.body.content_size,
        request.body.update_log_content_id.as_ref(),
        request.body.update_log_size,
    ) {
        (None, update_log_content_id, None) => {
            let mut content_ids = vec![request.body.content_id.to_string()];
            if let Some(update_log_content_id) = update_log_content_id {
                content_ids.push(update_log_content_id.to_string());
            }
            Ok(RelayPinRequestItems::Legacy(content_ids))
        }
        (Some(content_size), None, None) => {
            Ok(RelayPinRequestItems::Declared(vec![RelayPinItem {
                content_id: request.body.content_id.to_string(),
                size: content_size,
            }]))
        }
        (Some(content_size), Some(update_log_content_id), Some(update_log_size)) => {
            Ok(RelayPinRequestItems::Declared(vec![
                RelayPinItem {
                    content_id: request.body.content_id.to_string(),
                    size: content_size,
                },
                RelayPinItem {
                    content_id: update_log_content_id.to_string(),
                    size: update_log_size,
                },
            ]))
        }
        _ => Err(ApiError(NetworkError::InvalidInput(
            "invalid pin request: incomplete signed size declarations".to_string(),
        ))),
    }
}

pub async fn pin_status(
    State(state): State<AppState>,
    Path(content_id): Path<String>,
) -> Result<Json<RelayPinStatusResponse>, ApiError> {
    let status = state.daemon.status().await?;
    if !status.bootstrap_relay {
        return Err(ApiError(NetworkError::InvalidInput(
            "node is not configured to report relay pin status".to_string(),
        )));
    }

    let entry = state
        .daemon
        .list_cache_entries()
        .await?
        .into_iter()
        .find(|entry| entry.content_id == content_id);
    Ok(Json(RelayPinStatusResponse {
        content_id,
        pinned: entry.as_ref().map(|entry| entry.pinned).unwrap_or(false),
        size: entry.map(|entry| entry.size),
    }))
}
