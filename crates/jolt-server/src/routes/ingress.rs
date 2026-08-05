use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jolt_core::{
    verify_reachability_record_for_identity, IdentityId, JoltAddress, ReachabilityRecord,
    SIGNED_REACHABILITY_PATH,
};
use jolt_network::{DecryptedObjectResponse, IngressRecord};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::ApiError;
use crate::routes::app_api::{
    authenticated_session, require_capability, require_local_identity, AppApiError,
};
use crate::routes::fetch::fetch_error_for_target;
use crate::state::AppState;

#[derive(Debug, Deserialize, Serialize)]
pub struct SubmitIngressRequest {
    pub receiver_id: String,
    pub encrypted_object: Vec<u8>,
    pub expires_at: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SendIngressRequest {
    pub recipient: String,
    pub encrypted_object: Vec<u8>,
    pub expires_at: Option<u64>,
}

pub async fn submit_ingress(
    State(state): State<AppState>,
    Json(req): Json<SubmitIngressRequest>,
) -> Result<Json<IngressRecord>, ApiError> {
    let record = state
        .daemon
        .submit_ingress(req.receiver_id, req.encrypted_object, req.expires_at)
        .await?;
    Ok(Json(record))
}

pub async fn ingress_preflight() -> StatusCode {
    StatusCode::OK
}

pub async fn send_ingress_by_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<SendIngressRequest>,
) -> Result<Json<IngressRecord>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:send")?;

    let identity = recipient_identity(&req.recipient)?;

    // Self-delivery never touches the network: queue on our own daemon.
    // active_identity() renders the suffixed address form; compare bare ids.
    let local_identity = state.local_identities.active_identity().await;
    let sending_to_self = local_identity
        .as_deref()
        .map(|active| active.trim_end_matches(".jolt") == identity.to_string())
        .unwrap_or(false);
    if sending_to_self {
        let record = state
            .daemon
            .submit_ingress(
                "direct-live".to_string(),
                req.encrypted_object,
                req.expires_at,
            )
            .await?;
        return Ok(Json(record));
    }

    // Remote recipients are reached over p2p using the peer id their signed
    // reachability record declares. Reachability records advertise an HTTP
    // address too, but it is the daemon's own loopback bind and only ever
    // valid on the recipient's machine; POSTing to it from here delivered
    // envelopes to the sender's own daemon (#195).
    let endpoint = resolve_live_ingress_peer(&state, &identity, req.encrypted_object.len()).await?;
    let record = state
        .daemon
        .send_ingress_to_peer(
            endpoint,
            "p2p-live".to_string(),
            req.encrypted_object,
            req.expires_at,
        )
        .await?;

    // A delivery only counts when the daemon that queued it answers for the
    // identity we addressed. Anything else is a mis-delivery, never success.
    if record.recipient_identity != identity.to_string() {
        return Err(AppApiError::Network(
            jolt_network::NetworkError::Protocol(format!(
                "ingress envelope was accepted by {} instead of the addressed recipient",
                record.recipient_identity
            )),
        ));
    }
    Ok(Json(record))
}

pub async fn list_pending_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<IngressRecord>>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:read")?;
    let records = state.daemon.list_pending_ingress().await?;
    Ok(Json(records))
}

pub async fn open_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<DecryptedObjectResponse>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:read")?;
    let record = state.daemon.open_ingress(ingress_id).await?;
    Ok(Json(record))
}

pub async fn accept_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<IngressRecord>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:decide")?;
    let record = state.daemon.accept_ingress(ingress_id).await?;
    Ok(Json(record))
}

pub async fn reject_ingress(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(ingress_id): Path<String>,
) -> Result<Json<IngressRecord>, AppApiError> {
    let session = authenticated_session(&state, &headers).await?;
    require_local_identity(&state, &session).await?;
    require_capability(&session, "ingress:decide")?;
    let record = state.daemon.reject_ingress(ingress_id).await?;
    Ok(Json(record))
}

// Returns the peer id of the recipient's live ingress receiver, from its
// signed reachability record.
async fn resolve_live_ingress_peer(
    state: &AppState,
    identity: &IdentityId,
    object_bytes: usize,
) -> Result<String, AppApiError> {
    let address = JoltAddress::new(identity.clone(), SIGNED_REACHABILITY_PATH).map_err(|err| {
        AppApiError::Network(jolt_network::NetworkError::InvalidInput(err.to_string()))
    })?;
    let resolved = state.daemon.resolve(address.to_string()).await?;
    let fetched = state
        .daemon
        .fetch(resolved.content_id.clone())
        .await
        .map_err(|err| AppApiError::Network(fetch_error_for_target(err, &resolved.content_id)))?;
    let record: ReachabilityRecord = serde_json::from_slice(&fetched.data).map_err(|err| {
        AppApiError::Network(jolt_network::NetworkError::InvalidInput(err.to_string()))
    })?;
    let verified =
        verify_reachability_record_for_identity(identity, &record, unix_now()).map_err(|err| {
            AppApiError::Network(jolt_network::NetworkError::InvalidInput(err.to_string()))
        })?;

    verified
        .live
        .into_iter()
        .filter(|endpoint| {
            endpoint.transport == "jolt-http-ingress"
                && endpoint
                    .protocols
                    .iter()
                    .any(|protocol| protocol == "recipient-ingress-v1")
                && endpoint.max_payload_bytes >= object_bytes as u64
        })
        .map(|endpoint| endpoint.peer_id)
        .find(|peer_id| !peer_id.trim().is_empty())
        .ok_or_else(|| {
            AppApiError::Network(jolt_network::NetworkError::InvalidInput(
                "recipient has no usable live ingress receiver".to_string(),
            ))
        })
}

fn recipient_identity(raw: &str) -> Result<IdentityId, AppApiError> {
    match JoltAddress::from_str(raw) {
        Ok(address) => Ok(address.identity().clone()),
        Err(_) => IdentityId::from_str(raw.strip_suffix(".jolt").unwrap_or(raw)).map_err(|err| {
            AppApiError::Network(jolt_network::NetworkError::InvalidInput(format!(
                "invalid recipient identity {raw}: {err}"
            )))
        }),
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
