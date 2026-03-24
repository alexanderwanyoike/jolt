use std::collections::HashMap;
use std::time::{Duration, Instant};

use libp2p::request_response::OutboundRequestId;
use tokio::sync::oneshot;
use tracing::{info, warn};

use crate::command::FetchResult;
use crate::error::NetworkError;
use crate::protocol::ContentResponse;

/// Default timeout for fetch operations.
const DEFAULT_FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// How long to wait after provider connects before sending the content request.
/// This lets the relay circuit fully establish (based on real-world testing).
const RELAY_SETTLE_DELAY: Duration = Duration::from_secs(2);

/// Tracks the state of an in-flight fetch operation.
enum FetchState {
    /// Trying connected peers first. DHT query also running in parallel.
    TryingPeers {
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
        content_id: String,
        started_at: Instant,
        pending_requests: Vec<OutboundRequestId>,
        failed_count: usize,
        total_sent: usize,
    },
    /// Querying DHT for providers (no provider found yet).
    QueryingDht {
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
        content_id: String,
        started_at: Instant,
    },
    /// DHT found a provider. We've dialed them but waiting for:
    /// 1. The connection to establish (ConnectionEstablished event)
    /// 2. A 2s relay settle delay after connection
    WaitingForProvider {
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
        content_id: String,
        started_at: Instant,
        provider: libp2p::PeerId,
        /// Set when the provider connection is established
        connected_at: Option<Instant>,
    },
    /// Connected to provider, sent content request, waiting for response.
    FetchingFromProvider {
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
        content_id: String,
        started_at: Instant,
        request_id: OutboundRequestId,
    },
}

/// Manages in-flight fetch operations within the daemon event loop.
pub struct FetchManager {
    active: HashMap<String, FetchState>,
    request_to_content: HashMap<OutboundRequestId, String>,
    fetch_timeout: Duration,
}

impl FetchManager {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
            request_to_content: HashMap::new(),
            fetch_timeout: DEFAULT_FETCH_TIMEOUT,
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            active: HashMap::new(),
            request_to_content: HashMap::new(),
            fetch_timeout: timeout,
        }
    }

    /// Start a new fetch. peer_request_ids are requests already sent to connected peers.
    /// DHT query should be started separately by the caller.
    pub fn start_fetch(
        &mut self,
        content_id: String,
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
        peer_request_ids: Vec<OutboundRequestId>,
    ) {
        let now = Instant::now();

        if peer_request_ids.is_empty() {
            self.active.insert(
                content_id.clone(),
                FetchState::QueryingDht {
                    response_tx,
                    content_id,
                    started_at: now,
                },
            );
        } else {
            let total_sent = peer_request_ids.len();
            for &req_id in &peer_request_ids {
                self.request_to_content.insert(req_id, content_id.clone());
            }
            self.active.insert(
                content_id.clone(),
                FetchState::TryingPeers {
                    response_tx,
                    content_id,
                    started_at: now,
                    pending_requests: peer_request_ids,
                    failed_count: 0,
                    total_sent,
                },
            );
        }
    }

    /// Called when a successful content response arrives.
    /// Returns the content_id if this was a managed fetch (so the caller can cache it).
    pub fn on_content_response(
        &mut self,
        request_id: OutboundRequestId,
        response: ContentResponse,
    ) -> Option<String> {
        let Some(content_id) = self.request_to_content.remove(&request_id) else {
            return None;
        };

        let Some(state) = self.active.remove(&content_id) else {
            return None;
        };

        self.cleanup_request_mappings(&content_id);

        let response_tx = match state {
            FetchState::TryingPeers { response_tx, .. } => response_tx,
            FetchState::FetchingFromProvider { response_tx, .. } => response_tx,
            FetchState::QueryingDht { response_tx, .. } => response_tx,
            FetchState::WaitingForProvider { response_tx, .. } => response_tx,
        };

        let result = FetchResult {
            data: response.data.clone(),
            content_id: content_id.clone(),
            size: response.data.len() as u64,
        };

        info!("Fetch completed for {content_id} ({} bytes)", result.size);
        let _ = response_tx.send(Ok(result));
        Some(content_id)
    }

    /// Called when an outbound request fails.
    pub fn on_request_failure(&mut self, request_id: OutboundRequestId) -> bool {
        let Some(content_id) = self.request_to_content.remove(&request_id) else {
            return false;
        };

        let Some(state) = self.active.remove(&content_id) else {
            return false;
        };

        match state {
            FetchState::TryingPeers {
                response_tx,
                content_id,
                started_at,
                mut pending_requests,
                mut failed_count,
                total_sent,
            } => {
                pending_requests.retain(|id| *id != request_id);
                failed_count += 1;

                if failed_count >= total_sent {
                    // All peer requests failed, transition to DHT
                    info!("All peer requests failed for {content_id}, waiting for DHT providers");
                    self.active.insert(
                        content_id.clone(),
                        FetchState::QueryingDht {
                            response_tx,
                            content_id,
                            started_at,
                        },
                    );
                } else {
                    self.active.insert(
                        content_id.clone(),
                        FetchState::TryingPeers {
                            response_tx,
                            content_id,
                            started_at,
                            pending_requests,
                            failed_count,
                            total_sent,
                        },
                    );
                }
                true
            }
            FetchState::FetchingFromProvider {
                response_tx,
                content_id,
                ..
            } => {
                warn!("Provider fetch failed for {content_id}");
                let _ =
                    response_tx.send(Err(NetworkError::ProviderNotFound(content_id.clone())));
                true
            }
            FetchState::QueryingDht {
                response_tx,
                content_id,
                ..
            } => {
                let _ = response_tx.send(Err(NetworkError::ProviderNotFound(content_id)));
                true
            }
            FetchState::WaitingForProvider {
                response_tx,
                content_id,
                ..
            } => {
                let _ = response_tx.send(Err(NetworkError::ProviderNotFound(content_id)));
                true
            }
        }
    }

    /// Called when a DHT provider is discovered.
    /// Does NOT send a request -- just records the provider and transitions state.
    /// If `already_connected` is true, the relay settle timer starts immediately.
    /// Otherwise the caller should dial the provider and call `on_peer_connected`
    /// when the connection establishes.
    pub fn on_provider_discovered(
        &mut self,
        content_id: &str,
        provider: libp2p::PeerId,
        already_connected: bool,
    ) -> bool {
        let Some(state) = self.active.remove(content_id) else {
            return false;
        };

        match state {
            FetchState::QueryingDht {
                response_tx,
                content_id,
                started_at,
            }
            | FetchState::TryingPeers {
                response_tx,
                content_id,
                started_at,
                ..
            } => {
                let connected_at = if already_connected {
                    info!(
                        "Provider {provider} discovered for {content_id} (already connected)"
                    );
                    Some(Instant::now())
                } else {
                    info!(
                        "Provider {provider} discovered for {content_id}, waiting for connection"
                    );
                    None
                };
                self.active.insert(
                    content_id.clone(),
                    FetchState::WaitingForProvider {
                        response_tx,
                        content_id,
                        started_at,
                        provider,
                        connected_at,
                    },
                );
                true
            }
            other => {
                let key = Self::state_content_id(&other);
                self.active.insert(key, other);
                false
            }
        }
    }

    /// Called when a peer connection is established. If this peer is a provider
    /// we're waiting for, marks the connection time (starts the relay settle delay).
    pub fn on_peer_connected(&mut self, peer_id: &libp2p::PeerId) {
        for state in self.active.values_mut() {
            if let FetchState::WaitingForProvider {
                provider,
                connected_at,
                content_id,
                ..
            } = state
            {
                if provider == peer_id && connected_at.is_none() {
                    info!("Provider {peer_id} connected for {content_id}, waiting for relay to settle");
                    *connected_at = Some(Instant::now());
                }
            }
        }
    }

    /// Check if we have a fetch waiting for a DHT provider.
    pub fn is_awaiting_provider(&self, content_id: &str) -> bool {
        matches!(
            self.active.get(content_id),
            Some(FetchState::QueryingDht { .. }) | Some(FetchState::TryingPeers { .. })
        )
    }

    /// Returns content_ids + provider PeerIds that are ready to send content requests
    /// (connected and relay settle delay has passed).
    pub fn ready_to_request(&mut self) -> Vec<(String, libp2p::PeerId)> {
        let mut ready = Vec::new();

        for (content_id, state) in &self.active {
            if let FetchState::WaitingForProvider {
                connected_at: Some(connected_at),
                provider,
                ..
            } = state
            {
                if connected_at.elapsed() >= RELAY_SETTLE_DELAY {
                    ready.push((content_id.clone(), *provider));
                }
            }
        }

        ready
    }

    /// Transition a WaitingForProvider to FetchingFromProvider after the content
    /// request has been sent.
    pub fn mark_request_sent(
        &mut self,
        content_id: &str,
        request_id: OutboundRequestId,
    ) {
        let Some(state) = self.active.remove(content_id) else {
            return;
        };

        if let FetchState::WaitingForProvider {
            response_tx,
            content_id,
            started_at,
            ..
        } = state
        {
            self.request_to_content
                .insert(request_id, content_id.clone());
            self.active.insert(
                content_id.clone(),
                FetchState::FetchingFromProvider {
                    response_tx,
                    content_id,
                    started_at,
                    request_id,
                },
            );
        } else {
            let key = Self::state_content_id(&state);
            self.active.insert(key, state);
        }
    }

    /// Check for timed-out fetch operations.
    pub fn check_timeouts(&mut self) -> Vec<String> {
        let mut timed_out = Vec::new();

        for (content_id, state) in &self.active {
            let started_at = match state {
                FetchState::TryingPeers { started_at, .. } => started_at,
                FetchState::QueryingDht { started_at, .. } => started_at,
                FetchState::WaitingForProvider { started_at, .. } => started_at,
                FetchState::FetchingFromProvider { started_at, .. } => started_at,
            };
            if started_at.elapsed() > self.fetch_timeout {
                timed_out.push(content_id.clone());
            }
        }

        for content_id in &timed_out {
            if let Some(state) = self.active.remove(content_id) {
                self.cleanup_request_mappings(content_id);
                let response_tx = match state {
                    FetchState::TryingPeers { response_tx, .. } => response_tx,
                    FetchState::QueryingDht { response_tx, .. } => response_tx,
                    FetchState::WaitingForProvider { response_tx, .. } => response_tx,
                    FetchState::FetchingFromProvider { response_tx, .. } => response_tx,
                };
                warn!("Fetch timed out for {content_id}");
                let _ = response_tx.send(Err(NetworkError::Timeout));
            }
        }

        timed_out
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    fn cleanup_request_mappings(&mut self, content_id: &str) {
        self.request_to_content
            .retain(|_, cid| cid != content_id);
    }

    fn state_content_id(state: &FetchState) -> String {
        match state {
            FetchState::TryingPeers { content_id, .. } => content_id.clone(),
            FetchState::QueryingDht { content_id, .. } => content_id.clone(),
            FetchState::WaitingForProvider { content_id, .. } => content_id.clone(),
            FetchState::FetchingFromProvider { content_id, .. } => content_id.clone(),
        }
    }
}

impl Default for FetchManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_no_peers_goes_straight_to_dht() {
        let mut mgr = FetchManager::new();

        let (tx, _rx) = oneshot::channel();
        mgr.start_fetch("cid_no_peers".to_string(), tx, vec![]);

        assert_eq!(mgr.active_count(), 1);
        assert!(mgr.is_awaiting_provider("cid_no_peers"));
    }

    #[test]
    fn test_fetch_manager_timeout() {
        let mut mgr = FetchManager::with_timeout(Duration::from_millis(10));

        let (tx, mut rx) = oneshot::channel();
        mgr.start_fetch("cid_timeout".to_string(), tx, vec![]);
        assert_eq!(mgr.active_count(), 1);

        std::thread::sleep(Duration::from_millis(20));

        let timed_out = mgr.check_timeouts();
        assert_eq!(timed_out, vec!["cid_timeout".to_string()]);
        assert_eq!(mgr.active_count(), 0);

        let result = rx.try_recv().unwrap();
        assert!(matches!(result, Err(NetworkError::Timeout)));
    }

    #[test]
    fn test_fetch_manager_concurrent_no_peers() {
        let mut mgr = FetchManager::new();

        let (tx1, _rx1) = oneshot::channel();
        let (tx2, _rx2) = oneshot::channel();

        mgr.start_fetch("cid1".to_string(), tx1, vec![]);
        mgr.start_fetch("cid2".to_string(), tx2, vec![]);
        assert_eq!(mgr.active_count(), 2);

        assert!(mgr.is_awaiting_provider("cid1"));
        assert!(mgr.is_awaiting_provider("cid2"));
    }

    #[test]
    fn test_fetch_manager_concurrent_timeouts() {
        let mut mgr = FetchManager::with_timeout(Duration::from_millis(10));

        let (tx1, mut rx1) = oneshot::channel();
        let (tx2, mut rx2) = oneshot::channel();

        mgr.start_fetch("cid1".to_string(), tx1, vec![]);
        mgr.start_fetch("cid2".to_string(), tx2, vec![]);

        std::thread::sleep(Duration::from_millis(20));

        let timed_out = mgr.check_timeouts();
        assert_eq!(timed_out.len(), 2);
        assert_eq!(mgr.active_count(), 0);

        assert!(matches!(rx1.try_recv().unwrap(), Err(NetworkError::Timeout)));
        assert!(matches!(rx2.try_recv().unwrap(), Err(NetworkError::Timeout)));
    }

    #[test]
    fn test_provider_discovered_transitions_state() {
        let mut mgr = FetchManager::new();

        let (tx, _rx) = oneshot::channel();
        mgr.start_fetch("cid1".to_string(), tx, vec![]);
        assert!(mgr.is_awaiting_provider("cid1"));

        let provider = libp2p::PeerId::random();
        assert!(mgr.on_provider_discovered("cid1", provider, false));
        // No longer awaiting - now in WaitingForProvider
        assert!(!mgr.is_awaiting_provider("cid1"));
        assert_eq!(mgr.active_count(), 1);
        // Not ready to request yet (not connected)
        assert!(mgr.ready_to_request().is_empty());
    }

    #[test]
    fn test_provider_connection_then_settle() {
        let mut mgr = FetchManager::with_timeout(Duration::from_secs(30));

        let (tx, _rx) = oneshot::channel();
        mgr.start_fetch("cid1".to_string(), tx, vec![]);

        let provider = libp2p::PeerId::random();
        mgr.on_provider_discovered("cid1", provider, false);

        // Not ready yet (not connected)
        assert!(mgr.ready_to_request().is_empty());

        // Simulate connection
        mgr.on_peer_connected(&provider);

        // Still not ready (relay settle delay)
        assert!(mgr.ready_to_request().is_empty());

        // Wait for settle delay
        std::thread::sleep(RELAY_SETTLE_DELAY + Duration::from_millis(50));

        // Now ready
        let ready = mgr.ready_to_request();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, "cid1");
        assert_eq!(ready[0].1, provider);
    }
}
