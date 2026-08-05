use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod commands;
mod connectivity;
mod construction;
mod encryption;
mod events;
mod identity_heads;
mod ingress;
mod inventory;
mod providers;
mod publishing;
mod relays;
mod resolution;
mod status;
use libp2p::futures::StreamExt;
mod transport;

use libp2p::request_response::{self, OutboundRequestId};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, Swarm};
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use jolt_core::{
    verify_update_log_for_identity, ContentId, DeviceAuthorizationRecord, DeviceWriterLogEntry,
    EncryptedObjectEnvelope, IdentityEncryptionKey, IdentityEncryptionPrivateKey, IdentityId,
    JoltAddress, MergedDeviceIdentityState, ResolvedJoltTarget, UpdateLogEntry,
};
#[cfg(test)]
use jolt_core::{EncryptedObjectRecipient, IdentityHeadHint, RelayRecord, RelayRecordCapability};
use jolt_identity::NodeIdentity;
use jolt_store::ContentStore;

use crate::behaviour::{JoltBehaviour, JoltBehaviourEvent};
use crate::command::{
    DaemonCommand, IngressRecord, IngressStatus, RelayDiagnoseCacheObservation,
    RelayDiagnoseForwardingAttempt, RelayDiagnoseIdentityHeadObservation,
    RelayDiagnoseIdentityResponse, RelayDiagnoseProviderCandidate,
};
use crate::config::HomeRelayConfig;
use crate::error::{DiscoveryFailureCode, NetworkError};
use crate::fetch_manager::FetchManager;
use crate::node::identity_heads::IdentityHeadHintBook;
#[cfg(test)]
use crate::node::identity_heads::{
    IDENTITY_HEAD_HINT_EXCHANGE_MAX, IDENTITY_HEAD_HINT_MAX_PER_IDENTITY,
};
use crate::node::ingress::IngressQueue;
use crate::protocol::{
    ContentRequest, ContentResponse, IdentityProviderCandidate, RelayExchangeResponse,
    UpdateLogRequest, UpdateLogResponse,
};

struct PendingUpdateLogPin {
    identity: IdentityId,
    response_tx: oneshot::Sender<Result<u64, NetworkError>>,
}

struct CachedDeviceWriterState {
    authority_sequence: u64,
    merged: MergedDeviceIdentityState,
    /// The raw verified authority chain, retained so this node can re-serve the
    /// device-writer state to other nodes over the network and re-merge it when
    /// additional device logs arrive.
    authority_records: Vec<DeviceAuthorizationRecord>,
    /// The raw per-device writer logs, keyed by device id so a re-sync can
    /// merge newly discovered devices with already-known ones deterministically.
    device_logs: HashMap<String, Vec<DeviceWriterLogEntry>>,
}

struct PendingIdentityProviderForwardGroup {
    query_id: String,
    identity: IdentityId,
    providers: Vec<IdentityProviderCandidate>,
    channel: request_response::ResponseChannel<RelayExchangeResponse>,
    deadline: Instant,
    remaining_requests: usize,
    limit: usize,
}

struct PendingIdentityProviderDiagnosis {
    identity: IdentityId,
    local_update_log_cache: RelayDiagnoseCacheObservation,
    identity_head_hint: RelayDiagnoseIdentityHeadObservation,
    local_provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
    response_tx: oneshot::Sender<Result<RelayDiagnoseIdentityResponse, NetworkError>>,
    deadline: Instant,
    attempts: HashMap<OutboundRequestId, RelayDiagnoseForwardingAttempt>,
    providers: Vec<IdentityProviderCandidate>,
    limit: usize,
}

/// Tracks connection quality for a connected peer.
#[derive(Debug, Clone)]
pub struct PeerConnectionInfo {
    pub is_relayed: bool,
    pub transport: String,
    pub remote_addr: String,
}

impl PeerConnectionInfo {
    fn from_endpoint(endpoint: &libp2p::core::ConnectedPoint) -> Self {
        let (remote_addr, is_relayed) = match endpoint {
            libp2p::core::ConnectedPoint::Dialer { address, .. } => (
                address.to_string(),
                address.to_string().contains("p2p-circuit"),
            ),
            libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => (
                send_back_addr.to_string(),
                send_back_addr.to_string().contains("p2p-circuit"),
            ),
        };

        // With iroh transport, connections are QUIC-based
        let transport = if remote_addr.contains("quic") {
            "quic".to_string()
        } else if remote_addr.contains("/tcp/") {
            "tcp".to_string()
        } else {
            "iroh".to_string()
        };

        Self {
            is_relayed: is_relayed || endpoint.is_relayed(),
            transport,
            remote_addr,
        }
    }
}

pub struct NetworkNode {
    swarm: Swarm<JoltBehaviour>,
    identity: NodeIdentity,
    store: ContentStore,
    pending_fetches: HashMap<
        OutboundRequestId,
        (
            String,
            oneshot::Sender<Result<ContentResponse, NetworkError>>,
        ),
    >,
    pending_update_log_requests: HashMap<
        OutboundRequestId,
        (
            IdentityId,
            oneshot::Sender<Result<UpdateLogResponse, NetworkError>>,
        ),
    >,
    pending_update_log_pins: HashMap<OutboundRequestId, PendingUpdateLogPin>,
    pending_identity_provider_forwards: HashMap<OutboundRequestId, String>,
    pending_identity_provider_forward_groups: HashMap<String, PendingIdentityProviderForwardGroup>,
    pending_identity_provider_diagnostics: HashMap<String, PendingIdentityProviderDiagnosis>,
    pending_identity_provider_diagnostic_requests: HashMap<OutboundRequestId, String>,
    seen_identity_provider_queries: HashMap<String, Instant>,
    pending_jolt_resolutions: HashMap<
        OutboundRequestId,
        (
            JoltAddress,
            Option<u64>,
            libp2p::PeerId,
            oneshot::Sender<Result<ResolvedJoltTarget, NetworkError>>,
        ),
    >,
    pending_daemon_resolutions: HashMap<OutboundRequestId, resolution::PendingDaemonResolve>,
    pending_resolves: HashMap<IdentityId, Vec<resolution::PendingResolve>>,
    /// In-flight device-writer sync requests sent to providers, keyed by the
    /// outbound request id, so a response/failure can be routed back to the
    /// waiters that triggered the discovery.
    pending_device_writer_syncs: HashMap<OutboundRequestId, resolution::PendingDeviceWriterSync>,
    /// Device-writer sync waiters parked until a provider is discovered for an
    /// identity, keyed by identity.
    pending_device_writer_waiters: HashMap<IdentityId, Vec<resolution::DeviceWriterSyncWaiter>>,
    /// Providers discovered via DHT: content_id string -> provider PeerIds
    discovered_providers: HashMap<String, Vec<libp2p::PeerId>>,
    /// Verified update logs by owner identity.
    update_logs: HashMap<IdentityId, Vec<UpdateLogEntry>>,
    /// Verified merged device-writer state by owner identity.
    device_writer_states: HashMap<IdentityId, CachedDeviceWriterState>,
    /// Local per-device writer logs by owner identity.
    local_device_writer_logs: HashMap<IdentityId, Vec<DeviceWriterLogEntry>>,
    /// Local device authority records by owner identity.
    local_device_authority_records: HashMap<IdentityId, Vec<DeviceAuthorizationRecord>>,
    /// Signed, expiring identity-head hints learned through relay gossip.
    identity_head_hints: IdentityHeadHintBook,
    /// Connection quality tracking: peer -> connection info
    peer_connections: HashMap<libp2p::PeerId, PeerConnectionInfo>,
    /// When the node was created (for uptime reporting)
    started_at: Instant,
    /// Manages in-flight fetch operations for the daemon loop
    fetch_manager: FetchManager,
    /// Maximum time to wait for `.jolt` provider discovery.
    resolve_timeout: Duration,
    /// iroh endpoint for pre-populating peer addresses before dialing
    iroh_endpoint: Option<iroh::Endpoint>,
    /// Transport label reported through status endpoints.
    transport_name: &'static str,
    /// Bootstrap relays saved in persistent node config.
    configured_bootstrap_relays: Vec<String>,
    /// Bootstrap relays used for this daemon start.
    effective_bootstrap_relays: Vec<String>,
    /// Peer IDs parsed from effective bootstrap relay multiaddrs.
    bootstrap_peer_ids: HashSet<libp2p::PeerId>,
    /// Relay peers dialed from the relay address book for mesh exploration.
    relay_mesh_peer_ids: HashSet<libp2p::PeerId>,
    /// Cursor used to avoid asking every known relay during each exploration tick.
    relay_mesh_exploration_cursor: usize,
    /// Most recent bootstrap failure, if known.
    last_bootstrap_error: Option<String>,
    /// Whether this node is intentionally acting as a bootstrap/discovery relay.
    bootstrap_relay: bool,
    /// User-selected home relay for delegated availability.
    home_relay: Option<HomeRelayConfig>,
    /// Local X25519 keypair used to decrypt envelopes addressed to this identity.
    local_encryption_key: Option<(IdentityEncryptionKey, IdentityEncryptionPrivateKey)>,
    /// Whether this daemon start has published the local encryption key record.
    local_encryption_key_published: bool,
    /// Recipient-controlled envelopes received by this daemon and awaiting a decision.
    pending_ingress: IngressQueue,
    /// In-flight p2p ingress deliveries to remote recipients, keyed by the
    /// outbound request id of the /jolt/ingress/1.0.0 exchange.
    pending_ingress_submits: HashMap<OutboundRequestId, PendingIngressSubmit>,
}

/// An in-flight p2p ingress delivery. Carries everything needed to re-send:
/// idle-timeout closes and NAT path migrations kill connections between
/// exchanges, so the first attempt after a quiet period can race a dying
/// connection and fail with an IO error that a fresh dial resolves.
pub(crate) struct PendingIngressSubmit {
    peer: libp2p::PeerId,
    receiver_id: String,
    encrypted_object: Vec<u8>,
    expires_at: Option<u64>,
    attempts_left: u8,
    response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
}

/// Transient transport failures get this many automatic re-sends before the
/// error surfaces to the caller.
const INGRESS_SUBMIT_RETRIES: u8 = 2;

const RELAY_RECORD_TTL_SECS: u64 = 60 * 60;
const RELAY_EXCHANGE_MAX_RECORDS: usize = 32;
const RELAY_MESH_EXPLORATION_FANOUT: usize = 1;
const RELAY_MESH_EXPLORATION_INTERVAL: Duration = Duration::from_secs(1);
const IDENTITY_PROVIDER_QUERY_FANOUT: usize = 3;
const IDENTITY_PROVIDER_QUERY_LIMIT: u16 = 8;
const IDENTITY_PROVIDER_QUERY_TTL: u8 = 3;
const IDENTITY_PROVIDER_QUERY_TIMEOUT: Duration = Duration::from_secs(8);
const SEEN_IDENTITY_PROVIDER_QUERY_TTL: Duration = Duration::from_secs(30);

/// A daemon loop iteration slower than this starves queued API commands and is
/// worth naming in the logs (issue #187).
const SLOW_LOOP_BRANCH_THRESHOLD: Duration = Duration::from_millis(200);

impl NetworkNode {
    fn submit_ingress(
        &mut self,
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
    ) -> Result<IngressRecord, NetworkError> {
        if receiver_id.trim().is_empty() {
            return Err(NetworkError::InvalidInput(
                "receiver_id must not be empty".to_string(),
            ));
        }
        let now = unix_now();
        if expires_at.is_some_and(|expires_at| expires_at <= now) {
            return Err(NetworkError::InvalidInput(
                "ingress envelope is expired".to_string(),
            ));
        }
        let envelope = EncryptedObjectEnvelope::from_bytes(&encrypted_object)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        envelope
            .verify_signature()
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let local_identity = self.identity.identity_id();
        if !envelope
            .body
            .recipients
            .iter()
            .any(|recipient| recipient.recipient_identity == local_identity)
        {
            return Err(NetworkError::InvalidInput(
                "ingress envelope is not addressed to the local identity".to_string(),
            ));
        }

        let ingress_id = format!("ing_{}", ContentId::from_bytes(&encrypted_object));
        let record = IngressRecord {
            ingress_id,
            receiver_id,
            sender_identity: envelope.body.author.identity.to_string(),
            recipient_identity: local_identity.to_string(),
            schema_hint: envelope.body.plaintext.schema,
            status: IngressStatus::Pending,
            received_at: now,
            expires_at,
            size: encrypted_object.len() as u64,
            encrypted_object,
            accepted_at: None,
            rejected_at: None,
        };
        self.pending_ingress.push(record.clone());
        self.persist_ingress_queue();
        Ok(record)
    }

    /// Best-effort write-through of the ingress queue (#194). A failed write
    /// must not fail the submit/decide path; it only costs restart durability.
    fn persist_ingress_queue(&self) {
        if let Err(e) = self
            .store
            .save_ingress_queue(&self.pending_ingress.to_persisted())
        {
            tracing::warn!("Failed to persist ingress queue: {e}");
        }
    }

    fn list_pending_ingress(&self) -> Vec<IngressRecord> {
        self.pending_ingress.list_pending()
    }

    /// Deliver an ingress envelope to a remote recipient's daemon over the
    /// /jolt/ingress/1.0.0 request-response protocol. The result arrives via
    /// `response_tx` once the recipient answers (or the exchange fails). Dials
    /// the peer by id when not already connected; with the iroh transport a
    /// bare /p2p multiaddr resolves through discovery.
    pub fn send_ingress_to_peer(
        &mut self,
        peer: &libp2p::PeerId,
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
        response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
    ) {
        self.dispatch_ingress_submit(PendingIngressSubmit {
            peer: *peer,
            receiver_id,
            encrypted_object,
            expires_at,
            attempts_left: INGRESS_SUBMIT_RETRIES,
            response_tx,
        });
    }

    pub(crate) fn dispatch_ingress_submit(&mut self, pending: PendingIngressSubmit) {
        if !self.swarm.is_connected(&pending.peer) {
            let addr: libp2p::Multiaddr = format!("/p2p/{}", pending.peer)
                .parse()
                .expect("peer id renders a valid /p2p multiaddr");
            if let Err(e) = self.swarm.dial(addr) {
                tracing::debug!(
                    "Ingress delivery pre-dial to {} not started: {e}",
                    pending.peer
                );
            }
        }
        let request = crate::protocol::IngressSubmitRequest {
            receiver_id: pending.receiver_id.clone(),
            encrypted_object: pending.encrypted_object.clone(),
            expires_at: pending.expires_at,
        };
        let request_id = self
            .swarm
            .behaviour_mut()
            .ingress_submit
            .send_request(&pending.peer, request);
        self.pending_ingress_submits.insert(request_id, pending);
    }

    fn accept_ingress(&mut self, ingress_id: &str) -> Result<IngressRecord, NetworkError> {
        let record = self.pending_ingress.accept(ingress_id, unix_now())?;
        self.persist_ingress_queue();
        Ok(record)
    }

    fn open_ingress(
        &self,
        ingress_id: &str,
    ) -> Result<crate::command::DecryptedObjectResponse, NetworkError> {
        let encrypted_object = self.pending_ingress.encrypted_object(ingress_id)?;
        self.decrypt_object(encrypted_object)
    }

    fn reject_ingress(&mut self, ingress_id: &str) -> Result<IngressRecord, NetworkError> {
        let record = self.pending_ingress.reject(ingress_id, unix_now())?;
        self.persist_ingress_queue();
        Ok(record)
    }

    /// Request content from connected peers by ContentId.
    pub fn request_content(
        &mut self,
        content_id: &ContentId,
    ) -> Result<oneshot::Receiver<Result<ContentResponse, NetworkError>>, NetworkError> {
        let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
        if peers.is_empty() {
            return Err(NetworkError::NoPeers);
        }
        self.request_content_from(content_id, &peers[0])
    }

    /// Request content from a specific peer.
    pub fn request_content_from(
        &mut self,
        content_id: &ContentId,
        peer: &libp2p::PeerId,
    ) -> Result<oneshot::Receiver<Result<ContentResponse, NetworkError>>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let id_str = content_id.to_string();
        let request = ContentRequest {
            content_id: id_str.clone(),
        };

        let request_id = self
            .swarm
            .behaviour_mut()
            .content_fetch
            .send_request(peer, request);
        self.pending_fetches.insert(request_id, (id_str, tx));

        Ok(rx)
    }

    /// Request a signed update log for an identity from a specific peer.
    pub fn request_update_log_from(
        &mut self,
        identity: &IdentityId,
        since: Option<u64>,
        peer: &libp2p::PeerId,
    ) -> Result<oneshot::Receiver<Result<UpdateLogResponse, NetworkError>>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let request = UpdateLogRequest {
            identity: identity.clone(),
            since,
        };

        let request_id = self
            .swarm
            .behaviour_mut()
            .update_log_sync
            .send_request(peer, request);
        self.pending_update_log_requests
            .insert(request_id, (identity.clone(), tx));

        Ok(rx)
    }

    /// Resolve a Jolt address by requesting its identity update log from a specific peer.
    pub fn request_jolt_address_from_peer(
        &mut self,
        address: &JoltAddress,
        now: Option<u64>,
        peer: &libp2p::PeerId,
    ) -> Result<oneshot::Receiver<Result<ResolvedJoltTarget, NetworkError>>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        let request = UpdateLogRequest {
            identity: address.identity().clone(),
            since: self
                .update_logs
                .get(address.identity())
                .and_then(|entries| {
                    verify_update_log_for_identity(address.identity(), entries).ok()
                }),
        };

        let request_id = self
            .swarm
            .behaviour_mut()
            .update_log_sync
            .send_request(peer, request);
        self.pending_jolt_resolutions
            .insert(request_id, (address.clone(), now, *peer, tx));

        Ok(rx)
    }

    /// Get a reference to the content store.
    pub fn store(&self) -> &ContentStore {
        &self.store
    }

    /// Get a mutable reference to the content store.
    pub fn store_mut(&mut self) -> &mut ContentStore {
        &mut self.store
    }

    fn identity_provider_failure(
        identity: &IdentityId,
        no_bootstrap_relays: bool,
        relay_unreachable: bool,
    ) -> NetworkError {
        let key = Self::update_log_provider_key(identity);
        let (code, reason) = if no_bootstrap_relays {
            (
                DiscoveryFailureCode::NoBootstrapRelays,
                "this node has no bootstrap relays configured and no connected peers",
            )
        } else if relay_unreachable {
            (
                DiscoveryFailureCode::RelayUnreachable,
                "configured bootstrap relays are not reachable yet",
            )
        } else {
            (
                DiscoveryFailureCode::IdentityProviderNotFound,
                "the reachable relay mesh did not know an update-log provider for this identity",
            )
        };
        NetworkError::DiscoveryFailed {
            code,
            message: format!(
                "No update-log provider found for {key}: {reason}. A .jolt address is globally meaningful, but it is only reachable if this node can reach a relay mesh that knows where to find the identity."
            ),
        }
    }

    fn identity_head_invalid_failure(
        identity: &IdentityId,
        detail: impl Into<String>,
    ) -> NetworkError {
        NetworkError::DiscoveryFailed {
            code: DiscoveryFailureCode::IdentityHeadInvalid,
            message: format!(
                "Invalid identity head for {identity}: {}. The relay mesh found an update-log provider, but the returned signed identity state could not be verified.",
                detail.into()
            ),
        }
    }

    /// Run the daemon event loop, processing both swarm events and incoming commands.
    pub async fn run_daemon_loop(&mut self, mut cmd_rx: mpsc::Receiver<DaemonCommand>) {
        let mut timeout_interval = tokio::time::interval(Duration::from_secs(1));
        let mut relay_mesh_interval = tokio::time::interval(RELAY_MESH_EXPLORATION_INTERVAL);

        loop {
            while let Ok(command) = cmd_rx.try_recv() {
                if self.handle_daemon_loop_command(command) {
                    return;
                }
            }

            tokio::select! {
                biased;
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(command) => {
                            let started = Instant::now();
                            let shutdown = self.handle_daemon_loop_command(command);
                            Self::warn_slow_loop_branch(started, "command", None);
                            if shutdown {
                                return;
                            }
                        }
                        None => {
                            info!("Command channel closed, shutting down");
                            return;
                        }
                    }
                }
                event = self.swarm.select_next_some() => {
                    let summary = format!("{event:?}");
                    let started = Instant::now();
                    self.handle_swarm_event(event);
                    Self::warn_slow_loop_branch(started, "swarm", Some(&summary));
                }
                _ = timeout_interval.tick() => {
                    let started = Instant::now();
                    self.fetch_manager.check_timeouts();
                    self.check_resolve_timeouts();
                    self.check_device_writer_sync_timeouts();
                    self.check_identity_provider_forward_timeouts();
                    self.check_identity_diagnosis_timeouts();

                    // Send content requests for providers that have connected
                    let ready = self.fetch_manager.ready_to_request();
                    for (content_id, provider) in ready {
                        info!("Sending content request to provider {provider} for {content_id}");
                        let request = ContentRequest {
                            content_id: content_id.clone(),
                        };
                        let req_id = self
                            .swarm
                            .behaviour_mut()
                            .content_fetch
                            .send_request(&provider, request);
                        self.fetch_manager.mark_request_sent(&content_id, req_id);
                    }
                    Self::warn_slow_loop_branch(started, "timeout_tick", None);
                }
                _ = relay_mesh_interval.tick() => {
                    let started = Instant::now();
                    self.check_identity_provider_forward_timeouts();
                    self.check_identity_diagnosis_timeouts();
                    self.explore_relay_mesh();
                    Self::warn_slow_loop_branch(started, "relay_mesh_tick", None);
                }
            }
        }
    }

    /// Every daemon command waits behind the current loop iteration, so a
    /// handler that stalls for seconds starves the whole API (issue #187).
    /// Name the offending branch when that happens.
    fn warn_slow_loop_branch(started: Instant, branch: &str, detail: Option<&str>) {
        let elapsed = started.elapsed();
        if elapsed >= SLOW_LOOP_BRANCH_THRESHOLD {
            match detail {
                Some(detail) => {
                    let cut = detail.len().min(220);
                    tracing::warn!(
                        "Slow daemon loop branch: {branch} took {}ms handling {}",
                        elapsed.as_millis(),
                        &detail[..cut]
                    );
                }
                None => {
                    tracing::warn!(
                        "Slow daemon loop branch: {branch} took {}ms",
                        elapsed.as_millis()
                    );
                }
            }
        }
    }

    fn handle_daemon_loop_command(&mut self, command: DaemonCommand) -> bool {
        let should_shutdown = matches!(command, DaemonCommand::Shutdown { .. });
        self.handle_command(command);
        if should_shutdown {
            info!("Daemon shutting down");
        }
        should_shutdown
    }

    /// Run the event loop, processing swarm events until cancelled.
    pub async fn run_event_loop(&mut self) {
        let mut relay_mesh_interval = tokio::time::interval(RELAY_MESH_EXPLORATION_INTERVAL);
        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event);
                }
                _ = relay_mesh_interval.tick() => {
                    self.check_identity_provider_forward_timeouts();
                    self.check_identity_diagnosis_timeouts();
                    self.explore_relay_mesh();
                }
            }
        }
    }

    /// Poll for a single next swarm event.
    pub async fn next_event(&mut self) -> SwarmEvent<JoltBehaviourEvent> {
        self.swarm.select_next_some().await
    }

    /// Get the list of currently connected peers.
    pub fn connected_peers(&self) -> Vec<libp2p::PeerId> {
        self.swarm.connected_peers().cloned().collect()
    }

    /// Get the listeners' addresses.
    pub fn listeners(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    /// Set the fetch timeout for the daemon's fetch manager.
    pub fn set_fetch_timeout(&mut self, timeout: Duration) {
        self.fetch_manager = FetchManager::with_timeout(timeout);
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NetworkConfig;
    use jolt_core::{UpdateAction, UpdateLogEntry};
    use jolt_store::CacheConfig;
    use tempfile::tempdir;

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
    }

    fn make_node_with_identity(dir: &std::path::Path, identity: NodeIdentity) -> NetworkNode {
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
    }

    fn signed_profile_log(identity: &NodeIdentity, label: &[u8]) -> Vec<UpdateLogEntry> {
        vec![UpdateLogEntry::genesis(
            identity.public_key_bytes(),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: ContentId::from_bytes(label),
            },
            |bytes| identity.sign(bytes),
        )
        .unwrap()]
    }

    fn relay_record(identity: &NodeIdentity, observed_at: u64, expires_at: u64) -> RelayRecord {
        RelayRecord::new(
            identity.public_key_bytes(),
            identity.peer_id().to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            vec![
                RelayRecordCapability::Bootstrap,
                RelayRecordCapability::Discovery,
            ],
            observed_at,
            expires_at,
            |bytes| identity.sign(bytes),
        )
        .expect("test relay record should be valid")
    }

    fn identity_head_hint(
        identity: &NodeIdentity,
        provider: libp2p::PeerId,
        sequence: u64,
        observed_at: u64,
        expires_at: u64,
    ) -> IdentityHeadHint {
        IdentityHeadHint::new(
            identity.public_key_bytes(),
            identity.identity_id(),
            provider.to_string(),
            vec![format!("/ip4/127.0.0.1/tcp/4001/p2p/{provider}")],
            None,
            sequence,
            jolt_core::UpdateLogEntryHash([sequence as u8; 32]),
            observed_at,
            expires_at,
            |bytes| identity.sign(bytes),
        )
        .expect("test identity-head hint should be valid")
    }

    #[tokio::test]
    async fn local_identity_encryption_key_survives_node_restart() {
        let dir = tempdir().unwrap();
        let identity_dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        identity.save(identity_dir.path()).unwrap();

        let encrypted = {
            let mut node = make_node_with_identity(
                dir.path(),
                NodeIdentity::load(identity_dir.path()).unwrap(),
            );
            let local_key = node.ensure_local_identity_encryption_key().unwrap();
            node.encrypt_object(
                b"restart decrypts this".to_vec(),
                "text/plain".to_string(),
                vec![EncryptedObjectRecipient {
                    identity: node.identity.identity_id(),
                    key: local_key,
                }],
            )
            .unwrap()
            .data
        };

        let restarted =
            make_node_with_identity(dir.path(), NodeIdentity::load(identity_dir.path()).unwrap());
        let decrypted = restarted.decrypt_object(&encrypted).unwrap();

        assert_eq!(decrypted.plaintext, b"restart decrypts this");
    }

    #[test]
    fn identity_head_invalid_failure_is_structured() {
        let identity = NodeIdentity::generate().identity_id();

        let err = NetworkNode::identity_head_invalid_failure(&identity, "bad signature");

        match err {
            NetworkError::DiscoveryFailed { code, message } => {
                assert_eq!(code, DiscoveryFailureCode::IdentityHeadInvalid);
                assert!(message.contains(&identity.to_string()));
                assert!(message.contains("bad signature"));
            }
            other => panic!("expected structured discovery failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn local_identity_head_hint_requires_provider_addresses() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let identity_id = identity.identity_id();
        let update_log = signed_profile_log(&identity, b"addressless hint");
        let store = make_store(dir.path());
        let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

        node.store_verified_update_log(identity_id.clone(), update_log)
            .unwrap();

        assert!(node
            .identity_head_hints
            .get(&identity_id)
            .is_none_or(Vec::is_empty));
    }

    #[tokio::test]
    async fn node_stores_only_newer_verified_update_logs_for_identity() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let attacker = NodeIdentity::generate();
        let identity = owner.identity_id();
        let genesis = signed_profile_log(&owner, b"profile-v1")[0].clone();
        let newer = genesis
            .append(
                UpdateAction::SetPath {
                    path: "/profile".to_string(),
                    content_id: ContentId::from_bytes(b"profile-v2"),
                },
                |bytes| owner.sign(bytes),
            )
            .unwrap();
        let attacker_log = signed_profile_log(&attacker, b"attacker-profile");

        node.store_verified_update_log(identity.clone(), vec![genesis.clone()])
            .unwrap();
        node.store_verified_update_log(identity.clone(), attacker_log)
            .unwrap_err();
        let expected = vec![genesis.clone(), newer.clone()];

        node.store_verified_update_log(identity.clone(), expected.clone())
            .unwrap();
        node.store_verified_update_log(identity.clone(), vec![genesis])
            .unwrap();

        assert_eq!(
            node.update_log_entries(&identity),
            Some(expected.as_slice())
        );
    }

    #[tokio::test]
    async fn relay_exchange_rejects_invalid_records() {
        let dir = tempdir().unwrap();
        let node = make_node(dir.path());
        let identity = NodeIdentity::generate();
        let mut invalid = relay_record(&identity, 100, 200);
        invalid.body.addrs = vec!["/ip4/127.0.0.1/tcp/5001".to_string()];

        let (accepted, rejected) = node.store_relay_records(vec![invalid], 150);

        assert_eq!((accepted, rejected), (0, 1));
        assert_eq!(node.store().known_relay_count(150).unwrap(), 0);
    }

    #[tokio::test]
    async fn relay_exchange_is_bounded() {
        let dir = tempdir().unwrap();
        let node = make_node(dir.path());
        let records: Vec<_> = (0..40)
            .map(|index| {
                let identity = NodeIdentity::generate();
                relay_record(&identity, 100 + index, 300)
            })
            .collect();

        let (accepted, rejected) = node.store_relay_records(records, 150);

        assert_eq!((accepted, rejected), (RELAY_EXCHANGE_MAX_RECORDS as u16, 0));
        assert_eq!(
            node.store().known_relay_count(150).unwrap(),
            RELAY_EXCHANGE_MAX_RECORDS
        );
    }

    #[tokio::test]
    async fn identity_head_hints_reject_invalid_expired_and_stale_records() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let provider = libp2p::PeerId::random();
        let current = identity_head_hint(&owner, provider, 2, 100, 200);
        let stale = identity_head_hint(&owner, provider, 1, 100, 200);
        let expired = identity_head_hint(&owner, libp2p::PeerId::random(), 3, 100, 120);
        let mut tampered = identity_head_hint(&owner, libp2p::PeerId::random(), 4, 100, 200);
        tampered.body.latest_sequence = 5;

        let (accepted, rejected) =
            node.record_identity_head_hints(vec![current.clone(), stale, expired, tampered], 150);

        assert_eq!((accepted, rejected), (1, 3));
        assert_eq!(
            node.identity_head_hints
                .get(&owner.identity_id())
                .expect("fresh hint should be stored"),
            &vec![current]
        );
    }

    #[tokio::test]
    async fn identity_head_hints_are_bounded_per_identity() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let hints: Vec<_> = (0..8)
            .map(|index| identity_head_hint(&owner, libp2p::PeerId::random(), index, 100, 200))
            .collect();

        let (accepted, rejected) = node.record_identity_head_hints(hints, 150);

        assert_eq!((accepted, rejected), (8, 0));
        let stored = node
            .identity_head_hints
            .get(&owner.identity_id())
            .expect("identity hints should be stored");
        assert_eq!(stored.len(), IDENTITY_HEAD_HINT_MAX_PER_IDENTITY);
        assert_eq!(stored[0].body.latest_sequence, 7);
        assert_eq!(
            stored[IDENTITY_HEAD_HINT_MAX_PER_IDENTITY - 1]
                .body
                .latest_sequence,
            4
        );
    }

    #[tokio::test]
    async fn identity_head_hints_are_bounded_globally() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        for index in 0..8 {
            let owner = NodeIdentity::generate();
            let hint = identity_head_hint(&owner, libp2p::PeerId::random(), index as u64, 100, 200);
            node.record_identity_head_hints(vec![hint], 150);
        }
        node.identity_head_hints.enforce_global_bound_with_limit(4);

        assert_eq!(node.identity_head_hints.stored_count(), 4);
    }

    #[tokio::test]
    async fn identity_head_exchange_batch_is_fair_across_identities() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let dominant = NodeIdentity::generate();

        for sequence in 0..40 {
            let hint = identity_head_hint(&dominant, libp2p::PeerId::random(), sequence, 100, 300);
            node.record_identity_head_hints(vec![hint], 150);
        }

        for sequence in 0..4 {
            let owner = NodeIdentity::generate();
            let hint = identity_head_hint(&owner, libp2p::PeerId::random(), sequence, 100, 300);
            node.record_identity_head_hints(vec![hint], 150);
        }

        let hints = node.identity_head_hints_for_exchange(150, IDENTITY_HEAD_HINT_EXCHANGE_MAX);
        let identities: HashSet<_> = hints
            .iter()
            .map(|hint| hint.body.identity.clone())
            .collect();

        assert_eq!(hints.len(), 5);
        assert_eq!(identities.len(), 5);
        assert!(hints
            .iter()
            .any(|hint| hint.body.identity == dominant.identity_id()
                && hint.body.latest_sequence == 39));
    }

    #[tokio::test]
    async fn identity_head_exchange_batch_rotates_by_requested_limit() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        for sequence in 0..6 {
            let owner = NodeIdentity::generate();
            let hint = identity_head_hint(&owner, libp2p::PeerId::random(), sequence, 100, 300);
            node.record_identity_head_hints(vec![hint], 150);
        }

        let first = node.identity_head_hints_for_exchange(150, 2);
        let second = node.identity_head_hints_for_exchange(150, 2);
        let first_identities: HashSet<_> = first
            .iter()
            .map(|hint| hint.body.identity.clone())
            .collect();
        let second_identities: HashSet<_> = second
            .iter()
            .map(|hint| hint.body.identity.clone())
            .collect();

        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert!(first_identities.is_disjoint(&second_identities));
    }

    #[tokio::test]
    async fn publish_file_returns_valid_content_id() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"hello jolt").unwrap();

        let content_id = node.publish_file(&test_file).unwrap();
        assert!(content_id.verify(b"hello jolt"));
    }

    // --- Daemon command channel tests ---

    #[tokio::test]
    async fn test_daemon_handle_publish_fetch_roundtrip() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let test_file = dir.path().join("roundtrip.txt");
        std::fs::write(&test_file, b"roundtrip data").unwrap();
        let pub_resp = handle.publish(test_file, None).await.unwrap();

        let fetch_resp = handle.fetch(pub_resp.content_id.clone()).await.unwrap();
        assert_eq!(fetch_resp.data, b"roundtrip data");

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_handle_shutdown() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        handle.shutdown().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), daemon).await;
        assert!(result.is_ok());
    }
}
