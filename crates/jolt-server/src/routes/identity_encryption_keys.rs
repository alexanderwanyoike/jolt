use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::Json;
use jolt_core::{
    verify_identity_encryption_key_record_for_identity, IdentityEncryptionKey,
    IdentityEncryptionKeyRecord, IdentityId, JoltAddress, IDENTITY_ENCRYPTION_KEYS_PATH,
};
use jolt_network::NetworkError;
use serde::Serialize;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Serialize)]
pub struct IdentityEncryptionKeysResponse {
    pub identity: String,
    pub latest_sequence: u64,
    pub keys: Vec<IdentityEncryptionKey>,
}

pub async fn get_identity_encryption_keys(
    State(state): State<AppState>,
    Path(identity): Path<String>,
) -> Result<Json<IdentityEncryptionKeysResponse>, ApiError> {
    let identity = IdentityId::from_str(&identity)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let address = JoltAddress::new(identity.clone(), IDENTITY_ENCRYPTION_KEYS_PATH)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let resolved = state.daemon.resolve(address.to_string()).await?;
    let fetched = state.daemon.fetch(resolved.content_id).await?;
    let record: IdentityEncryptionKeyRecord = serde_json::from_slice(&fetched.data)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let verified = verify_identity_encryption_key_record_for_identity(&identity, &record, now)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;

    Ok(Json(IdentityEncryptionKeysResponse {
        identity: verified.identity.to_string(),
        latest_sequence: verified.latest_sequence,
        keys: verified.keys,
    }))
}
