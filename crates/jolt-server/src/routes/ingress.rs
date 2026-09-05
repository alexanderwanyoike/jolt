use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use jolt_core::{
    verify_reachability_record_for_identity, IdentityId, JoltAddress, ReachabilityRecord,
    ReachabilityRecordError, VerifiedReachability, SIGNED_REACHABILITY_PATH,
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
    let verified = resolve_with_bounded_refresh(
        || resolve_verified_reachability(state, identity),
        || async {
            if let Err(err) = state
                .daemon
                .refresh_materialized_record_view(
                    identity.clone(),
                    SIGNED_REACHABILITY_PATH.to_string(),
                )
                .await
            {
                tracing::debug!("reachability refresh for {identity} skipped: {err}");
            }
        },
    )
    .await?;

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

enum ResolveAttempt<T, E> {
    Ready(T),
    Expired(E),
    Failed(E),
}

// `daemon.resolve` answers from cache and refreshes in the background, so a
// sender can hold a recipient's expired record for a while after the recipient
// has already renewed it. One bounded refresh followed by a second resolve
// closes that window. Validation is never relaxed: a record that is still
// expired after the refresh is reported as expired.
async fn resolve_with_bounded_refresh<T, E, R, RF, Rf, RfF>(
    mut resolve: R,
    refresh: Rf,
) -> Result<T, E>
where
    R: FnMut() -> RF,
    RF: std::future::Future<Output = ResolveAttempt<T, E>>,
    Rf: FnOnce() -> RfF,
    RfF: std::future::Future<Output = ()>,
{
    match resolve().await {
        ResolveAttempt::Ready(value) => return Ok(value),
        ResolveAttempt::Failed(err) => return Err(err),
        ResolveAttempt::Expired(_) => {}
    }
    refresh().await;
    match resolve().await {
        ResolveAttempt::Ready(value) => Ok(value),
        ResolveAttempt::Expired(err) | ResolveAttempt::Failed(err) => Err(err),
    }
}

async fn resolve_verified_reachability(
    state: &AppState,
    identity: &IdentityId,
) -> ResolveAttempt<VerifiedReachability, AppApiError> {
    fn invalid(err: impl std::fmt::Display) -> AppApiError {
        AppApiError::Network(jolt_network::NetworkError::InvalidInput(err.to_string()))
    }
    let address = match JoltAddress::new(identity.clone(), SIGNED_REACHABILITY_PATH) {
        Ok(address) => address,
        Err(err) => return ResolveAttempt::Failed(invalid(err)),
    };
    let resolved = match state.daemon.resolve(address.to_string()).await {
        Ok(resolved) => resolved,
        Err(err) => return ResolveAttempt::Failed(AppApiError::Network(err)),
    };
    let fetched = match state.daemon.fetch(resolved.content_id.clone()).await {
        Ok(fetched) => fetched,
        Err(err) => {
            return ResolveAttempt::Failed(AppApiError::Network(fetch_error_for_target(
                err,
                &resolved.content_id,
            )))
        }
    };
    let record: ReachabilityRecord = match serde_json::from_slice(&fetched.data) {
        Ok(record) => record,
        Err(err) => return ResolveAttempt::Failed(invalid(err)),
    };
    match verify_reachability_record_for_identity(identity, &record, unix_now()) {
        Ok(verified) => ResolveAttempt::Ready(verified),
        Err(err @ ReachabilityRecordError::ExpiredRecord) => ResolveAttempt::Expired(invalid(err)),
        Err(err) => ResolveAttempt::Failed(invalid(err)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn scripted<'a>(
        script: &'a RefCell<Vec<ResolveAttempt<&'static str, &'static str>>>,
    ) -> impl FnMut() -> std::future::Ready<ResolveAttempt<&'static str, &'static str>> + 'a {
        move || std::future::ready(script.borrow_mut().remove(0))
    }

    fn counting<'a>(refreshes: &'a Cell<u32>) -> impl FnOnce() -> std::future::Ready<()> + 'a {
        move || {
            refreshes.set(refreshes.get() + 1);
            std::future::ready(())
        }
    }

    #[tokio::test]
    async fn fresh_record_never_refreshes() {
        let script = RefCell::new(vec![ResolveAttempt::Ready("peer")]);
        let refreshes = Cell::new(0);

        let result = resolve_with_bounded_refresh(scripted(&script), counting(&refreshes)).await;

        assert_eq!(result, Ok("peer"));
        assert_eq!(refreshes.get(), 0);
    }

    #[tokio::test]
    async fn expired_cache_refreshes_once_and_uses_the_renewed_record() {
        let script = RefCell::new(vec![
            ResolveAttempt::Expired("expired"),
            ResolveAttempt::Ready("peer"),
        ]);
        let refreshes = Cell::new(0);

        let result = resolve_with_bounded_refresh(scripted(&script), counting(&refreshes)).await;

        assert_eq!(result, Ok("peer"));
        assert_eq!(refreshes.get(), 1);
    }

    #[tokio::test]
    async fn still_expired_after_refresh_fails_with_the_expiry_error() {
        let script = RefCell::new(vec![
            ResolveAttempt::Expired("expired"),
            ResolveAttempt::Expired("expired"),
        ]);
        let refreshes = Cell::new(0);

        let result = resolve_with_bounded_refresh(scripted(&script), counting(&refreshes)).await;

        assert_eq!(result, Err("expired"));
        assert_eq!(refreshes.get(), 1);
        assert!(
            script.borrow().is_empty(),
            "exactly two resolves, never a third"
        );
    }

    #[tokio::test]
    async fn non_expiry_failures_are_not_retried() {
        let script = RefCell::new(vec![ResolveAttempt::Failed("bad signature")]);
        let refreshes = Cell::new(0);

        let result = resolve_with_bounded_refresh(scripted(&script), counting(&refreshes)).await;

        assert_eq!(result, Err("bad signature"));
        assert_eq!(refreshes.get(), 0);
    }
}
