use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{Path, State};
use axum::Json;
use jolt_core::{
    verify_reachability_record_for_identity, IdentityId, JoltAddress, LiveReachabilityEndpoint,
    OfflineIngressEndpoint, ReachabilityRecord, VerifiedReachability, SIGNED_REACHABILITY_PATH,
};
use jolt_network::{NetworkError, PublishReachabilityResponse};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PublishReachabilityRequest {
    pub sequence_hint: u64,
    pub expires_at: u64,
    pub live: Vec<LiveReachabilityEndpoint>,
    pub offline_ingress: Vec<OfflineIngressEndpoint>,
}

#[derive(Debug, Serialize)]
pub struct ReachabilityResponse {
    pub identity: String,
    pub sequence_hint: u64,
    pub expires_at: u64,
    pub live: Vec<LiveReachabilityEndpoint>,
    pub offline_ingress: Vec<OfflineIngressEndpoint>,
}

pub async fn publish_local_reachability(
    State(state): State<AppState>,
    Json(req): Json<PublishReachabilityRequest>,
) -> Result<Json<PublishReachabilityResponse>, ApiError> {
    let response = state
        .daemon
        .publish_reachability(
            req.sequence_hint,
            req.expires_at,
            req.live,
            req.offline_ingress,
        )
        .await?;
    Ok(Json(response))
}

pub async fn get_identity_reachability(
    State(state): State<AppState>,
    Path(identity): Path<String>,
) -> Result<Json<ReachabilityResponse>, ApiError> {
    let identity = IdentityId::from_str(&identity)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let address = JoltAddress::new(identity.clone(), SIGNED_REACHABILITY_PATH)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let resolved = state.daemon.resolve(address.to_string()).await?;
    let fetched = state.daemon.fetch(resolved.content_id).await?;
    let record: ReachabilityRecord = serde_json::from_slice(&fetched.data)
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;
    let verified = verify_reachability_record_for_identity(&identity, &record, unix_now())
        .map_err(|err| ApiError(NetworkError::InvalidInput(err.to_string())))?;

    Ok(Json(reachability_response(verified)))
}

fn reachability_response(verified: VerifiedReachability) -> ReachabilityResponse {
    ReachabilityResponse {
        identity: verified.identity.to_string(),
        sequence_hint: verified.sequence_hint,
        expires_at: verified.expires_at,
        live: verified.live,
        offline_ingress: verified.offline_ingress,
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
