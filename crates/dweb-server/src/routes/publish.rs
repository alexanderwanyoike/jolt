use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use axum_extra::extract::Multipart;
use serde_json::json;

use crate::error::ApiError;
use crate::state::AppState;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub async fn publish_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ApiError> {
    // Extract the file from multipart
    let mut file_data = None;
    let mut path = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        match field.name() {
            Some("file") => match field.bytes().await {
                Ok(bytes) => {
                    file_data = Some(bytes);
                }
                Err(e) => {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("Failed to read file: {e}") })),
                    )
                        .into_response());
                }
            },
            Some("path") => match field.text().await {
                Ok(value) if !value.trim().is_empty() => {
                    path = Some(value);
                }
                Ok(_) => {}
                Err(e) => {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("Failed to read path: {e}") })),
                    )
                        .into_response());
                }
            },
            _ => {}
        }
    }

    let data = match file_data {
        Some(d) => d,
        None => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "No file field in multipart request" })),
            )
                .into_response());
        }
    };

    // Write to a temp file for the daemon to publish
    let temp_dir = std::env::temp_dir();
    let unique_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_path = temp_dir.join(format!(
        "dweb_publish_{}_{unique_id}_{sequence}",
        std::process::id(),
    ));
    std::fs::write(&temp_path, &data).map_err(|e| ApiError(dweb_network::NetworkError::Io(e)))?;

    let result = state.daemon.publish(temp_path.clone(), path).await;

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    match result {
        Ok(response) => Ok((StatusCode::OK, Json(json!(response))).into_response()),
        Err(e) => Err(ApiError(e)),
    }
}
