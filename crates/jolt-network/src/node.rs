use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use libp2p::futures::StreamExt;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm, Transport};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use jolt_core::{
    decrypt_encrypted_object_for_recipient, generate_identity_encryption_keypair,
    resolve_jolt_address, verify_update_log_for_identity, ContentId, ContentManifest,
    EncryptedObjectEnvelope, EncryptedObjectRecipient, IdentityEncryptionKey,
    IdentityEncryptionKeyRecord, IdentityEncryptionPrivateKey, IdentityHeadHint, IdentityId,
    JoltAddress, LiveReachabilityEndpoint, OfflineIngressEndpoint, PinRequest, ReachabilityRecord,
    RelayRecord, RelayRecordCapability, ResolvedJoltTarget, UpdateAction, UpdateLogEntry,
    VerifiedReachability, IDENTITY_ENCRYPTION_KEYS_PATH, SIGNED_REACHABILITY_PATH,
};
use jolt_identity::NodeIdentity;
use jolt_store::{ContentStore, HomeRelayPinRecord, LocalIdentityEncryptionKeypair};

use crate::behaviour::{JoltBehaviour, JoltBehaviourEvent};
use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, FetchResult, IngressRecord, IngressStatus,
    NodeStatus, PeerConnectResponse, PeerInfo, PublishReachabilityResponse, PublishResponse,
    PublishedContentInfo, PublishedRelayInfo, RelayDiagnoseCacheObservation,
    RelayDiagnoseForwarding, RelayDiagnoseForwardingAttempt, RelayDiagnoseIdentityHeadObservation,
    RelayDiagnoseIdentityResponse, RelayDiagnoseOutcome, RelayDiagnoseProviderCandidate,
    ResolveResponse,
};
use crate::config::{HomeRelayConfig, NetworkConfig};
use crate::error::{DiscoveryFailureCode, NetworkError};
use crate::fetch_manager::FetchManager;
use crate::protocol::{
    ContentRequest, ContentResponse, IdentityProviderCandidate, RelayExchangeRequest,
    RelayExchangeResponse, UpdateLogRequest, UpdateLogResponse,
};

struct PendingResolve {
    address: JoltAddress,
    response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    deadline: Instant,
    fallback_response: Option<ResolveResponse>,
}

struct PendingUpdateLogPin {
    identity: IdentityId,
    response_tx: oneshot::Sender<Result<u64, NetworkError>>,
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

fn relay_record_matches_capabilities(
    record: &RelayRecord,
    capabilities: &[RelayRecordCapability],
) -> bool {
    capabilities.is_empty()
        || capabilities
            .iter()
            .any(|capability| record.body.capabilities.contains(capability))
}

impl From<IdentityProviderCandidate> for RelayDiagnoseProviderCandidate {
    fn from(candidate: IdentityProviderCandidate) -> Self {
        Self {
            peer_id: candidate.peer_id,
            addrs: candidate.addrs,
        }
    }
}

fn dedupe_diagnose_provider_candidates(
    candidates: Vec<RelayDiagnoseProviderCandidate>,
    limit: usize,
) -> Vec<RelayDiagnoseProviderCandidate> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.peer_id.clone()) {
            deduped.push(candidate);
            if deduped.len() >= limit {
                break;
            }
        }
    }
    deduped
}

struct PendingDaemonResolve {
    address: JoltAddress,
    now: Option<u64>,
    provider: libp2p::PeerId,
    response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    deadline: Instant,
    fallback_response: Option<ResolveResponse>,
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
    pending_daemon_resolutions: HashMap<OutboundRequestId, PendingDaemonResolve>,
    pending_resolves: HashMap<IdentityId, Vec<PendingResolve>>,
    /// Providers discovered via DHT: content_id string -> provider PeerIds
    discovered_providers: HashMap<String, Vec<libp2p::PeerId>>,
    /// Verified update logs by owner identity.
    update_logs: HashMap<IdentityId, Vec<UpdateLogEntry>>,
    /// Signed, expiring identity-head hints learned through relay gossip.
    identity_head_hints: HashMap<IdentityId, Vec<IdentityHeadHint>>,
    /// Cursor used to rotate bounded identity-head gossip batches across identities.
    identity_head_gossip_cursor: usize,
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
    pending_ingress: Vec<IngressRecord>,
}

const RELAY_RECORD_TTL_SECS: u64 = 60 * 60;
const RELAY_EXCHANGE_MAX_RECORDS: usize = 32;
const RELAY_MESH_EXPLORATION_FANOUT: usize = 1;
const RELAY_MESH_EXPLORATION_INTERVAL: Duration = Duration::from_secs(1);
const IDENTITY_PROVIDER_QUERY_FANOUT: usize = 3;
const IDENTITY_PROVIDER_QUERY_LIMIT: u16 = 8;
const IDENTITY_PROVIDER_QUERY_TTL: u8 = 3;
const IDENTITY_PROVIDER_QUERY_TIMEOUT: Duration = Duration::from_secs(8);
const SEEN_IDENTITY_PROVIDER_QUERY_TTL: Duration = Duration::from_secs(30);
const IDENTITY_HEAD_HINT_MAX_GLOBAL: usize = 1024;
const IDENTITY_HEAD_HINT_MAX_PER_IDENTITY: usize = 4;
const IDENTITY_HEAD_HINT_EXCHANGE_MAX: usize = 32;

impl NetworkNode {
    /// Create a new network node with iroh transport.
    pub async fn new(
        identity: NodeIdentity,
        store: ContentStore,
        config: NetworkConfig,
    ) -> Result<Self, NetworkError> {
        let libp2p_keypair = identity.to_libp2p_keypair();
        let peer_id = libp2p_keypair.public().to_peer_id();

        // Create iroh transport (handles NAT traversal, DERP relay, hole punching)
        let transport = if config.p2p_port > 0 {
            info!(
                "Binding iroh transport to fixed UDP port {}",
                config.p2p_port
            );
            libp2p_iroh::Transport::new_with_port(Some(&libp2p_keypair), config.p2p_port)
                .await
                .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
        } else {
            libp2p_iroh::Transport::new(Some(&libp2p_keypair))
                .await
                .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
        };

        // Get the iroh endpoint for pre-populating peer addresses
        let iroh_endpoint = transport.endpoint().ok();

        // Build behaviours (only 4 -- iroh handles all NAT/relay)
        let mdns = if config.enable_mdns {
            Some(
                libp2p::mdns::tokio::Behaviour::new(libp2p::mdns::Config::default(), peer_id)
                    .map_err(|e| NetworkError::Swarm(e.to_string()))?,
            )
        } else {
            None
        }
        .into();

        let content_fetch = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let update_log_sync = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/update-log/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let relay_exchange = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/relays/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(StreamProtocol::new("/jolt/kad/1.0.0"));
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
            "/jolt/id/1.0.0".to_string(),
            libp2p_keypair.public(),
        ));

        let behaviour = JoltBehaviour {
            mdns,
            content_fetch,
            update_log_sync,
            relay_exchange,
            kademlia,
            identify,
        };

        let swarm = Swarm::new(
            transport.boxed(),
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(Duration::from_secs(300)),
        );

        let update_logs = Self::load_persisted_local_update_log(&store, &identity)?;

        let bootstrap_peer_ids = Self::parse_bootstrap_peer_ids(&config.effective_bootstrap_relays);
        let local_encryption_key = Self::load_persisted_local_encryption_key(&store, &identity)?;

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            pending_update_log_requests: HashMap::new(),
            pending_update_log_pins: HashMap::new(),
            pending_identity_provider_forwards: HashMap::new(),
            pending_identity_provider_forward_groups: HashMap::new(),
            pending_identity_provider_diagnostics: HashMap::new(),
            pending_identity_provider_diagnostic_requests: HashMap::new(),
            seen_identity_provider_queries: HashMap::new(),
            pending_jolt_resolutions: HashMap::new(),
            pending_daemon_resolutions: HashMap::new(),
            pending_resolves: HashMap::new(),
            discovered_providers: HashMap::new(),
            update_logs,
            identity_head_hints: HashMap::new(),
            identity_head_gossip_cursor: 0,
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
            resolve_timeout: Duration::from_secs(10),
            iroh_endpoint,
            transport_name: "iroh",
            configured_bootstrap_relays: config.configured_bootstrap_relays,
            effective_bootstrap_relays: config.effective_bootstrap_relays,
            bootstrap_peer_ids,
            relay_mesh_peer_ids: HashSet::new(),
            relay_mesh_exploration_cursor: 0,
            last_bootstrap_error: None,
            bootstrap_relay: config.bootstrap_relay,
            home_relay: config.home_relay,
            local_encryption_key,
            local_encryption_key_published: false,
            pending_ingress: Vec::new(),
        })
    }

    /// Create a new network node with TCP transport (for testing in isolated namespaces).
    ///
    /// Uses noise + yamux over TCP instead of iroh, so it works in environments
    /// without internet access (e.g. patchbay network namespaces).
    pub fn new_tcp(
        identity: NodeIdentity,
        store: ContentStore,
        config: NetworkConfig,
    ) -> Result<Self, NetworkError> {
        let libp2p_keypair = identity.to_libp2p_keypair();
        let peer_id = libp2p_keypair.public().to_peer_id();

        let transport = libp2p::tcp::tokio::Transport::default()
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(
                libp2p::noise::Config::new(&libp2p_keypair)
                    .map_err(|e| NetworkError::Swarm(e.to_string()))?,
            )
            .multiplex(libp2p::yamux::Config::default())
            .boxed();

        let mdns = if config.enable_mdns {
            Some(
                libp2p::mdns::tokio::Behaviour::new(libp2p::mdns::Config::default(), peer_id)
                    .map_err(|e| NetworkError::Swarm(e.to_string()))?,
            )
        } else {
            None
        }
        .into();

        let content_fetch = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let update_log_sync = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/update-log/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let relay_exchange = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/jolt/relays/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(StreamProtocol::new("/jolt/kad/1.0.0"));
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
            "/jolt/id/1.0.0".to_string(),
            libp2p_keypair.public(),
        ));

        let behaviour = JoltBehaviour {
            mdns,
            content_fetch,
            update_log_sync,
            relay_exchange,
            kademlia,
            identify,
        };

        let swarm = Swarm::new(
            transport,
            behaviour,
            peer_id,
            libp2p::swarm::Config::with_tokio_executor()
                .with_idle_connection_timeout(Duration::from_secs(300)),
        );

        let update_logs = Self::load_persisted_local_update_log(&store, &identity)?;

        let bootstrap_peer_ids = Self::parse_bootstrap_peer_ids(&config.effective_bootstrap_relays);
        let local_encryption_key = Self::load_persisted_local_encryption_key(&store, &identity)?;

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            pending_update_log_requests: HashMap::new(),
            pending_update_log_pins: HashMap::new(),
            pending_identity_provider_forwards: HashMap::new(),
            pending_identity_provider_forward_groups: HashMap::new(),
            pending_identity_provider_diagnostics: HashMap::new(),
            pending_identity_provider_diagnostic_requests: HashMap::new(),
            seen_identity_provider_queries: HashMap::new(),
            pending_jolt_resolutions: HashMap::new(),
            pending_daemon_resolutions: HashMap::new(),
            pending_resolves: HashMap::new(),
            discovered_providers: HashMap::new(),
            update_logs,
            identity_head_hints: HashMap::new(),
            identity_head_gossip_cursor: 0,
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
            resolve_timeout: Duration::from_secs(10),
            iroh_endpoint: None,
            transport_name: "tcp",
            configured_bootstrap_relays: config.configured_bootstrap_relays,
            effective_bootstrap_relays: config.effective_bootstrap_relays,
            bootstrap_peer_ids,
            relay_mesh_peer_ids: HashSet::new(),
            relay_mesh_exploration_cursor: 0,
            last_bootstrap_error: None,
            bootstrap_relay: config.bootstrap_relay,
            home_relay: config.home_relay,
            local_encryption_key,
            local_encryption_key_published: false,
            pending_ingress: Vec::new(),
        })
    }

    fn parse_bootstrap_peer_ids(relays: &[String]) -> HashSet<libp2p::PeerId> {
        relays
            .iter()
            .filter_map(|addr| crate::bootstrap::parse_bootstrap_addr(addr).ok())
            .map(|(peer_id, _)| peer_id)
            .collect()
    }

    /// Start listening on a multiaddr.
    pub fn listen_on(&mut self, addr: &str) -> Result<Multiaddr, NetworkError> {
        let multiaddr: Multiaddr = addr
            .parse()
            .map_err(|e: libp2p::multiaddr::Error| NetworkError::Swarm(e.to_string()))?;
        self.swarm
            .listen_on(multiaddr.clone())
            .map_err(|e| NetworkError::Swarm(e.to_string()))?;
        Ok(multiaddr)
    }

    /// Dial a specific peer address.
    pub fn dial(&mut self, addr: Multiaddr) -> Result<(), NetworkError> {
        self.swarm
            .dial(addr)
            .map_err(|e| NetworkError::Swarm(e.to_string()))?;
        Ok(())
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &libp2p::PeerId {
        self.swarm.local_peer_id()
    }

    /// Build this daemon's current signed relay record when relay mode is enabled.
    pub fn local_relay_record(&self, now: u64) -> Result<Option<RelayRecord>, NetworkError> {
        if !self.bootstrap_relay {
            return Ok(None);
        }

        let addrs = self
            .swarm
            .listeners()
            .map(|addr| addr.to_string())
            .collect();
        let record = RelayRecord::new(
            self.identity.public_key_bytes(),
            self.swarm.local_peer_id().to_string(),
            addrs,
            vec![
                RelayRecordCapability::Bootstrap,
                RelayRecordCapability::Discovery,
                RelayRecordCapability::Pinning,
            ],
            now,
            now + RELAY_RECORD_TTL_SECS,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()));

        record.map(Some)
    }

    fn start_relay_record_exchange(&mut self, peer: &libp2p::PeerId) {
        let now = unix_now();
        let mut known_records =
            self.relay_records_for_exchange(RELAY_EXCHANGE_MAX_RECORDS, &[], now);

        let announce_records: Vec<_> = known_records
            .drain(..)
            .filter(|record| record.body.peer_id != peer.to_string())
            .take(RELAY_EXCHANGE_MAX_RECORDS)
            .collect();

        if !announce_records.is_empty() {
            self.swarm.behaviour_mut().relay_exchange.send_request(
                peer,
                RelayExchangeRequest::AnnounceRelays {
                    records: announce_records,
                },
            );
        }

        let announce_hints =
            self.identity_head_hints_for_exchange(now, IDENTITY_HEAD_HINT_EXCHANGE_MAX);
        if !announce_hints.is_empty() {
            self.swarm.behaviour_mut().relay_exchange.send_request(
                peer,
                RelayExchangeRequest::AnnounceIdentityHeads {
                    hints: announce_hints,
                },
            );
        }

        self.swarm.behaviour_mut().relay_exchange.send_request(
            peer,
            RelayExchangeRequest::GetRelays {
                limit: RELAY_EXCHANGE_MAX_RECORDS as u16,
                capabilities: Vec::new(),
            },
        );

        self.swarm.behaviour_mut().relay_exchange.send_request(
            peer,
            RelayExchangeRequest::GetIdentityHeads {
                limit: IDENTITY_HEAD_HINT_EXCHANGE_MAX as u16,
            },
        );
    }

    fn relay_records_for_exchange(
        &self,
        limit: usize,
        capabilities: &[RelayRecordCapability],
        now: u64,
    ) -> Vec<RelayRecord> {
        let mut records: Vec<RelayRecord> = self
            .store
            .load_relay_records(now)
            .map(|records| {
                records
                    .into_iter()
                    .map(|stored| stored.relay_record)
                    .collect()
            })
            .unwrap_or_else(|e| {
                debug!("Failed to load relay address book for exchange: {e}");
                Vec::new()
            });

        if let Ok(Some(local_record)) = self.local_relay_record(now) {
            records.push(local_record);
        }

        records.retain(|record| relay_record_matches_capabilities(record, capabilities));
        records.sort_by_key(|record| record.body.relay_id.to_string());
        records.dedup_by(|a, b| a.body.relay_id == b.body.relay_id);
        records.truncate(limit.min(RELAY_EXCHANGE_MAX_RECORDS));
        records
    }

    fn store_relay_records(&self, records: Vec<RelayRecord>, now: u64) -> (u16, u16) {
        let mut accepted = 0u16;
        let mut rejected = 0u16;
        for record in records.into_iter().take(RELAY_EXCHANGE_MAX_RECORDS) {
            match self.store.record_relay_record(record, now) {
                Ok(()) => accepted = accepted.saturating_add(1),
                Err(e) => {
                    debug!("Rejected exchanged relay record: {e}");
                    rejected = rejected.saturating_add(1);
                }
            }
        }
        (accepted, rejected)
    }

    fn identity_head_hints_for_exchange(
        &mut self,
        now: u64,
        limit: usize,
    ) -> Vec<IdentityHeadHint> {
        let mut hints_by_identity: Vec<_> = self
            .identity_head_hints
            .iter()
            .filter_map(|(identity, hints)| {
                let best = hints
                    .iter()
                    .filter(|hint| hint.verify_at(now).is_ok())
                    .max_by(|a, b| {
                        a.body
                            .latest_sequence
                            .cmp(&b.body.latest_sequence)
                            .then_with(|| a.body.expires_at.cmp(&b.body.expires_at))
                            .then_with(|| a.body.provider_peer_id.cmp(&b.body.provider_peer_id))
                    })?
                    .clone();
                Some((identity.to_string(), best))
            })
            .collect();

        hints_by_identity.sort_by(|a, b| a.0.cmp(&b.0));
        if hints_by_identity.is_empty() {
            self.identity_head_gossip_cursor = 0;
            return Vec::new();
        }

        let limit = limit
            .min(IDENTITY_HEAD_HINT_EXCHANGE_MAX)
            .min(hints_by_identity.len());
        let start = self.identity_head_gossip_cursor % hints_by_identity.len();
        let mut selected = Vec::with_capacity(limit);
        for offset in 0..hints_by_identity.len() {
            if selected.len() >= limit {
                break;
            }
            let index = (start + offset) % hints_by_identity.len();
            selected.push(hints_by_identity[index].1.clone());
        }
        self.identity_head_gossip_cursor = (start + selected.len()) % hints_by_identity.len();
        selected
    }

    pub fn record_identity_head_hint(
        &mut self,
        hint: IdentityHeadHint,
    ) -> Result<(), NetworkError> {
        let now = unix_now();
        hint.verify_at(now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        self.record_identity_head_hints(vec![hint], now);
        Ok(())
    }

    fn record_identity_head_hints(&mut self, hints: Vec<IdentityHeadHint>, now: u64) -> (u16, u16) {
        self.prune_identity_head_hints(now);
        let mut accepted = 0u16;
        let mut rejected = 0u16;

        for hint in hints.into_iter().take(IDENTITY_HEAD_HINT_EXCHANGE_MAX) {
            if let Err(e) = hint.verify_at(now) {
                debug!("Rejected identity-head hint: {e}");
                rejected = rejected.saturating_add(1);
                continue;
            }

            let identity = hint.body.identity.clone();
            let provider = hint.body.provider_peer_id.clone();
            let entry = self.identity_head_hints.entry(identity).or_default();
            if let Some(existing) = entry
                .iter_mut()
                .find(|existing| existing.body.provider_peer_id == provider)
            {
                if hint.body.latest_sequence <= existing.body.latest_sequence {
                    rejected = rejected.saturating_add(1);
                    continue;
                }
                *existing = hint;
            } else {
                entry.push(hint);
            }

            entry.sort_by(|a, b| {
                b.body
                    .latest_sequence
                    .cmp(&a.body.latest_sequence)
                    .then_with(|| b.body.expires_at.cmp(&a.body.expires_at))
            });
            entry.truncate(IDENTITY_HEAD_HINT_MAX_PER_IDENTITY);
            accepted = accepted.saturating_add(1);
        }

        self.enforce_identity_head_hint_global_bound();
        (accepted, rejected)
    }

    fn prune_identity_head_hints(&mut self, now: u64) {
        self.identity_head_hints.retain(|_, hints| {
            hints.retain(|hint| hint.verify_at(now).is_ok());
            !hints.is_empty()
        });
    }

    fn enforce_identity_head_hint_global_bound(&mut self) {
        self.enforce_identity_head_hint_global_bound_with_limit(IDENTITY_HEAD_HINT_MAX_GLOBAL);
    }

    fn enforce_identity_head_hint_global_bound_with_limit(&mut self, limit: usize) {
        let mut ranked = Vec::new();
        for (identity, hints) in &self.identity_head_hints {
            for hint in hints {
                ranked.push((
                    identity.clone(),
                    hint.body.provider_peer_id.clone(),
                    hint.body.latest_sequence,
                    hint.body.expires_at,
                ));
            }
        }
        if ranked.len() <= limit {
            return;
        }

        ranked.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.3.cmp(&a.3)));
        let keep: HashSet<_> = ranked
            .into_iter()
            .take(limit)
            .map(|(identity, provider, _, _)| (identity, provider))
            .collect();
        self.identity_head_hints.retain(|identity, hints| {
            hints.retain(|hint| {
                keep.contains(&(identity.clone(), hint.body.provider_peer_id.clone()))
            });
            !hints.is_empty()
        });
    }

    fn relay_record_peer_addr(record: &RelayRecord) -> Option<(libp2p::PeerId, Multiaddr)> {
        let peer_id = libp2p::PeerId::from_str(&record.body.peer_id).ok()?;
        let addr = record.body.addrs.first()?.parse::<Multiaddr>().ok()?;
        let addr = if addr
            .iter()
            .any(|protocol| matches!(protocol, Protocol::P2p(_)))
        {
            addr
        } else {
            addr.with(Protocol::P2p(peer_id))
        };
        Some((peer_id, addr))
    }

    fn mark_relay_peer_success(&self, peer: &libp2p::PeerId, now: u64) {
        if let Err(e) = self
            .store
            .mark_relay_record_peer_success(&peer.to_string(), now)
        {
            debug!("Failed to mark relay peer success for {peer}: {e}");
        }
    }

    fn mark_relay_peer_failure(&self, peer: &libp2p::PeerId) {
        if let Err(e) = self.store.mark_relay_record_peer_failure(&peer.to_string()) {
            debug!("Failed to mark relay peer failure for {peer}: {e}");
        }
    }

    fn explore_relay_mesh(&mut self) {
        if !self.bootstrap_relay {
            return;
        }

        let now = unix_now();
        let records = match self.store.load_relay_records(now) {
            Ok(records) => records,
            Err(e) => {
                debug!("Failed to load relay records for mesh exploration: {e}");
                return;
            }
        };
        if records.is_empty() {
            return;
        }

        let local_peer = *self.swarm.local_peer_id();
        let start = self.relay_mesh_exploration_cursor % records.len();
        let mut explored = 0usize;

        for offset in 0..records.len() {
            if explored >= RELAY_MESH_EXPLORATION_FANOUT {
                break;
            }

            let index = (start + offset) % records.len();
            self.relay_mesh_exploration_cursor = (index + 1) % records.len();
            let record = &records[index].relay_record;
            let Some((peer_id, addr)) = Self::relay_record_peer_addr(record) else {
                continue;
            };
            if peer_id == local_peer {
                continue;
            }

            explored += 1;
            self.relay_mesh_peer_ids.insert(peer_id);
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr.clone());

            if self.swarm.is_connected(&peer_id) {
                self.start_relay_record_exchange(&peer_id);
                continue;
            }

            debug!("Exploring relay mesh through {peer_id} at {addr}");
            if let Err(e) = self.swarm.dial(addr) {
                debug!("Failed to dial relay mesh peer {peer_id}: {e}");
                self.mark_relay_peer_failure(&peer_id);
            }
        }
    }

    fn local_identity_provider_candidate(&self) -> IdentityProviderCandidate {
        let peer_id = self.swarm.local_peer_id().to_string();
        let addrs = self
            .swarm
            .listeners()
            .map(|addr| {
                if addr
                    .iter()
                    .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                {
                    addr.to_string()
                } else {
                    addr.clone()
                        .with(Protocol::P2p(*self.swarm.local_peer_id()))
                        .to_string()
                }
            })
            .collect();
        IdentityProviderCandidate { peer_id, addrs }
    }

    fn relay_record_candidate_for_peer(
        &self,
        peer: &libp2p::PeerId,
        now: u64,
    ) -> Option<IdentityProviderCandidate> {
        let peer_id = peer.to_string();
        let records = self.store.load_relay_records(now).ok()?;
        records
            .into_iter()
            .find(|record| record.relay_record.body.peer_id == peer_id)
            .map(|record| {
                let addrs = record
                    .relay_record
                    .body
                    .addrs
                    .into_iter()
                    .map(|addr| {
                        if addr.contains("/p2p/") {
                            addr
                        } else {
                            format!("{addr}/p2p/{peer_id}")
                        }
                    })
                    .collect();
                IdentityProviderCandidate { peer_id, addrs }
            })
    }

    fn identity_provider_candidates(
        &self,
        identity: &IdentityId,
        limit: usize,
        now: u64,
    ) -> Vec<IdentityProviderCandidate> {
        let mut candidates = Vec::new();
        if self.update_logs.contains_key(identity) {
            candidates.push(self.local_identity_provider_candidate());
        }

        let key = Self::update_log_provider_key(identity);
        if let Some(providers) = self.discovered_providers.get(&key) {
            for provider in providers {
                if let Some(candidate) = self.relay_record_candidate_for_peer(provider, now) {
                    candidates.push(candidate);
                } else {
                    candidates.push(IdentityProviderCandidate {
                        peer_id: provider.to_string(),
                        addrs: Vec::new(),
                    });
                }
            }
        }

        if let Some(hints) = self.identity_head_hints.get(identity) {
            for hint in hints {
                if hint.verify_at(now).is_err() {
                    continue;
                }
                candidates.push(IdentityProviderCandidate {
                    peer_id: hint.body.provider_peer_id.clone(),
                    addrs: hint.body.provider_addrs.clone(),
                });
            }
        }

        Self::dedupe_identity_provider_candidates(candidates, limit)
    }

    fn dedupe_identity_provider_candidates(
        candidates: Vec<IdentityProviderCandidate>,
        limit: usize,
    ) -> Vec<IdentityProviderCandidate> {
        let mut seen = HashSet::new();
        let mut deduped = Vec::new();
        for candidate in candidates {
            if seen.insert(candidate.peer_id.clone()) {
                deduped.push(candidate);
                if deduped.len() >= limit {
                    break;
                }
            }
        }
        deduped
    }

    fn relay_query_peers(&self, excluded: Option<&libp2p::PeerId>) -> Vec<libp2p::PeerId> {
        self.swarm
            .connected_peers()
            .filter(|peer| Some(*peer) != excluded)
            .filter(|peer| {
                self.bootstrap_peer_ids.contains(peer) || self.relay_mesh_peer_ids.contains(peer)
            })
            .take(IDENTITY_PROVIDER_QUERY_FANOUT)
            .copied()
            .collect()
    }

    fn identity_provider_query_id(&self, identity: &IdentityId) -> String {
        format!(
            "{}:{}:{}",
            self.swarm.local_peer_id(),
            identity,
            unix_now_ms()
        )
    }

    fn diagnose_identity(
        &mut self,
        identity: IdentityId,
        response_tx: oneshot::Sender<Result<RelayDiagnoseIdentityResponse, NetworkError>>,
    ) {
        let now = unix_now();
        let local_update_log_cache = self.diagnose_update_log_cache(&identity);
        let identity_head_hint = self.diagnose_identity_head_hints(&identity, now);
        let local_provider_candidates: Vec<_> = self
            .identity_provider_candidates(&identity, IDENTITY_PROVIDER_QUERY_LIMIT as usize, now)
            .into_iter()
            .map(Into::into)
            .collect();
        let relay_targets = self.relay_query_peers(None);
        let provider_candidates = local_provider_candidates.clone();
        if !provider_candidates.is_empty() || relay_targets.is_empty() {
            let response = self.build_identity_diagnosis_response(
                &identity,
                local_update_log_cache,
                identity_head_hint,
                local_provider_candidates,
                provider_candidates,
                Vec::new(),
            );
            let _ = response_tx.send(Ok(response));
            return;
        }

        let query_id = self.identity_provider_query_id(&identity);
        let request = RelayExchangeRequest::FindIdentityProviders {
            query_id: query_id.clone(),
            identity: identity.clone(),
            limit: IDENTITY_PROVIDER_QUERY_LIMIT,
            ttl: IDENTITY_PROVIDER_QUERY_TTL,
            deadline_unix_ms: unix_now_ms() + self.resolve_timeout.as_millis() as u64,
        };
        let mut attempts = HashMap::new();
        for peer in relay_targets {
            let request_id = self
                .swarm
                .behaviour_mut()
                .relay_exchange
                .send_request(&peer, request.clone());
            self.pending_identity_provider_diagnostic_requests
                .insert(request_id, query_id.clone());
            attempts.insert(
                request_id,
                RelayDiagnoseForwardingAttempt {
                    peer_id: peer.to_string(),
                    status: "pending".to_string(),
                    candidate_count: 0,
                    error: None,
                },
            );
        }

        self.pending_identity_provider_diagnostics.insert(
            query_id,
            PendingIdentityProviderDiagnosis {
                identity,
                local_update_log_cache,
                identity_head_hint,
                local_provider_candidates,
                response_tx,
                deadline: Instant::now() + self.resolve_timeout,
                attempts,
                providers: Vec::new(),
                limit: IDENTITY_PROVIDER_QUERY_LIMIT as usize,
            },
        );
    }

    fn build_identity_diagnosis_response(
        &self,
        identity: &IdentityId,
        local_update_log_cache: RelayDiagnoseCacheObservation,
        identity_head_hint: RelayDiagnoseIdentityHeadObservation,
        local_provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
        provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
        attempts: Vec<RelayDiagnoseForwardingAttempt>,
    ) -> RelayDiagnoseIdentityResponse {
        RelayDiagnoseIdentityResponse {
            identity: identity.to_string(),
            provider_key: Self::update_log_provider_key(identity),
            local_update_log_cache,
            identity_head_hint,
            local_provider_candidates,
            provider_candidates: provider_candidates.clone(),
            relay_forwarding: RelayDiagnoseForwarding {
                attempted: !attempts.is_empty(),
                target_count: attempts.len(),
                attempts,
            },
            outcome: self.diagnose_identity_outcome(identity, !provider_candidates.is_empty()),
        }
    }

    fn diagnose_update_log_cache(&self, identity: &IdentityId) -> RelayDiagnoseCacheObservation {
        let Some(entries) = self.update_logs.get(identity) else {
            return RelayDiagnoseCacheObservation {
                state: "miss".to_string(),
                entry_count: 0,
                latest_sequence: None,
            };
        };

        let latest_sequence = verify_update_log_for_identity(identity, entries).ok();
        RelayDiagnoseCacheObservation {
            state: if latest_sequence.is_some() {
                "hit".to_string()
            } else {
                "invalid".to_string()
            },
            entry_count: entries.len(),
            latest_sequence,
        }
    }

    fn diagnose_identity_head_hints(
        &self,
        identity: &IdentityId,
        now: u64,
    ) -> RelayDiagnoseIdentityHeadObservation {
        let Some(hints) = self.identity_head_hints.get(identity) else {
            return RelayDiagnoseIdentityHeadObservation {
                state: "miss".to_string(),
                fresh_count: 0,
                expired_count: 0,
                latest_sequence: None,
                provider_peer_id: None,
                expires_at: None,
            };
        };

        let mut fresh = Vec::new();
        let mut expired_count = 0;
        for hint in hints {
            if hint.verify_at(now).is_ok() {
                fresh.push(hint);
            } else {
                expired_count += 1;
            }
        }
        let best = fresh.iter().max_by_key(|hint| hint.body.latest_sequence);

        RelayDiagnoseIdentityHeadObservation {
            state: if fresh.is_empty() {
                "miss".to_string()
            } else {
                "hit".to_string()
            },
            fresh_count: fresh.len(),
            expired_count,
            latest_sequence: best.map(|hint| hint.body.latest_sequence),
            provider_peer_id: best.map(|hint| hint.body.provider_peer_id.clone()),
            expires_at: best.map(|hint| hint.body.expires_at),
        }
    }

    fn diagnose_identity_outcome(
        &self,
        identity: &IdentityId,
        has_provider_candidates: bool,
    ) -> RelayDiagnoseOutcome {
        if has_provider_candidates {
            return RelayDiagnoseOutcome {
                code: "provider_candidates_found".to_string(),
                message: "Provider candidates are known for this identity.".to_string(),
            };
        }

        let no_bootstrap_relays = self.effective_bootstrap_relays.is_empty()
            && self.swarm.connected_peers().next().is_none();
        let relay_unreachable = !self.effective_bootstrap_relays.is_empty()
            && self.connected_bootstrap_peer_count() == 0;

        match Self::identity_provider_failure(identity, no_bootstrap_relays, relay_unreachable) {
            NetworkError::DiscoveryFailed { code, message } => RelayDiagnoseOutcome {
                code: code.as_str().to_string(),
                message,
            },
            other => RelayDiagnoseOutcome {
                code: "diagnose_failed".to_string(),
                message: other.to_string(),
            },
        }
    }

    fn handle_identity_diagnosis_response(
        &mut self,
        request_id: OutboundRequestId,
        providers: Vec<IdentityProviderCandidate>,
    ) -> bool {
        let Some(query_id) = self
            .pending_identity_provider_diagnostic_requests
            .remove(&request_id)
        else {
            return false;
        };

        let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id) else {
            return true;
        };

        if let Some(attempt) = pending.attempts.get_mut(&request_id) {
            attempt.status = "responded".to_string();
            attempt.candidate_count = providers.len();
        }
        pending.providers.extend(providers);

        if self.identity_diagnosis_should_finish(&pending) {
            self.finish_identity_diagnosis(pending);
        } else {
            self.pending_identity_provider_diagnostics
                .insert(query_id, pending);
        }

        true
    }

    fn handle_identity_diagnosis_failure(
        &mut self,
        request_id: OutboundRequestId,
        error: String,
    ) -> bool {
        let Some(query_id) = self
            .pending_identity_provider_diagnostic_requests
            .remove(&request_id)
        else {
            return false;
        };

        let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id) else {
            return true;
        };

        if let Some(attempt) = pending.attempts.get_mut(&request_id) {
            attempt.status = "failed".to_string();
            attempt.error = Some(error);
        }

        if self.identity_diagnosis_should_finish(&pending) {
            self.finish_identity_diagnosis(pending);
        } else {
            self.pending_identity_provider_diagnostics
                .insert(query_id, pending);
        }

        true
    }

    fn identity_diagnosis_should_finish(&self, pending: &PendingIdentityProviderDiagnosis) -> bool {
        pending
            .attempts
            .values()
            .all(|attempt| attempt.status != "pending")
            || pending.providers.len() >= pending.limit
    }

    fn finish_identity_diagnosis(&mut self, pending: PendingIdentityProviderDiagnosis) {
        for request_id in pending.attempts.keys() {
            self.pending_identity_provider_diagnostic_requests
                .remove(request_id);
        }

        let mut provider_candidates = pending.local_provider_candidates.clone();
        provider_candidates.extend(
            Self::dedupe_identity_provider_candidates(pending.providers, pending.limit)
                .into_iter()
                .map(Into::into),
        );
        provider_candidates =
            dedupe_diagnose_provider_candidates(provider_candidates, pending.limit);

        let response = self.build_identity_diagnosis_response(
            &pending.identity,
            pending.local_update_log_cache,
            pending.identity_head_hint,
            pending.local_provider_candidates,
            provider_candidates,
            pending.attempts.into_values().collect(),
        );
        let _ = pending.response_tx.send(Ok(response));
    }

    fn check_identity_diagnosis_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<_> = self
            .pending_identity_provider_diagnostics
            .iter()
            .filter_map(|(query_id, pending)| (pending.deadline <= now).then_some(query_id.clone()))
            .collect();

        for query_id in timed_out {
            if let Some(mut pending) = self.pending_identity_provider_diagnostics.remove(&query_id)
            {
                for attempt in pending.attempts.values_mut() {
                    if attempt.status == "pending" {
                        attempt.status = "timeout".to_string();
                        attempt.error = Some("relay forwarding timed out".to_string());
                    }
                }
                self.finish_identity_diagnosis(pending);
            }
        }
    }

    fn start_identity_provider_relay_query(&mut self, identity: &IdentityId) {
        let query_id = self.identity_provider_query_id(identity);
        let request = RelayExchangeRequest::FindIdentityProviders {
            query_id,
            identity: identity.clone(),
            limit: IDENTITY_PROVIDER_QUERY_LIMIT,
            ttl: IDENTITY_PROVIDER_QUERY_TTL,
            deadline_unix_ms: unix_now_ms() + IDENTITY_PROVIDER_QUERY_TIMEOUT.as_millis() as u64,
        };

        for peer in self.relay_query_peers(None) {
            self.swarm
                .behaviour_mut()
                .relay_exchange
                .send_request(&peer, request.clone());
        }
    }

    fn record_identity_provider_candidates(
        &mut self,
        identity: &IdentityId,
        providers: Vec<IdentityProviderCandidate>,
    ) -> Vec<libp2p::PeerId> {
        let key = Self::update_log_provider_key(identity);
        let mut accepted = Vec::new();

        for provider in providers {
            let Ok(peer_id) = libp2p::PeerId::from_str(&provider.peer_id) else {
                continue;
            };
            if peer_id == *self.swarm.local_peer_id() {
                continue;
            }

            for addr in provider.addrs {
                if let Ok(addr) = addr.parse::<Multiaddr>() {
                    let addr = if addr
                        .iter()
                        .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                    {
                        addr
                    } else {
                        addr.with(Protocol::P2p(peer_id))
                    };
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    if let Err(e) = self.swarm.dial(addr) {
                        debug!("Failed to dial identity provider candidate {peer_id}: {e}");
                    }
                }
            }

            let providers = self.discovered_providers.entry(key.clone()).or_default();
            if !providers.contains(&peer_id) {
                providers.push(peer_id);
                accepted.push(peer_id);
            }
        }

        accepted
    }

    fn prune_seen_identity_provider_queries(&mut self) {
        let now = Instant::now();
        self.seen_identity_provider_queries
            .retain(|_, seen_at| now.duration_since(*seen_at) < SEEN_IDENTITY_PROVIDER_QUERY_TTL);
    }

    fn handle_identity_provider_query(
        &mut self,
        peer: libp2p::PeerId,
        query_id: String,
        identity: IdentityId,
        limit: u16,
        ttl: u8,
        deadline_unix_ms: u64,
        channel: request_response::ResponseChannel<RelayExchangeResponse>,
    ) {
        self.prune_seen_identity_provider_queries();
        let now_ms = unix_now_ms();
        let limit = usize::from(limit.min(IDENTITY_PROVIDER_QUERY_LIMIT));
        let providers = self.identity_provider_candidates(&identity, limit, unix_now());
        let already_seen = self
            .seen_identity_provider_queries
            .insert(query_id.clone(), Instant::now())
            .is_some();
        debug!(
            "Identity provider query {query_id} for {identity}: {} local candidates, ttl={ttl}",
            providers.len()
        );

        if already_seen
            || !providers.is_empty()
            || ttl == 0
            || deadline_unix_ms <= now_ms
            || providers.len() >= limit
        {
            self.respond_identity_providers(channel, query_id, identity, providers, limit);
            return;
        }

        let next_peers = self.relay_query_peers(Some(&peer));
        if next_peers.is_empty() {
            self.respond_identity_providers(channel, query_id, identity, providers, limit);
            return;
        }
        debug!(
            "Forwarding identity provider query {query_id} for {identity} to {} relay(s) with ttl={}",
            next_peers.len(),
            ttl.saturating_sub(1)
        );

        let remaining_requests = next_peers.len();
        for next_peer in next_peers {
            let request_id = self.swarm.behaviour_mut().relay_exchange.send_request(
                &next_peer,
                RelayExchangeRequest::FindIdentityProviders {
                    query_id: query_id.clone(),
                    identity: identity.clone(),
                    limit: limit as u16,
                    ttl: ttl.saturating_sub(1),
                    deadline_unix_ms,
                },
            );
            self.pending_identity_provider_forwards
                .insert(request_id, query_id.clone());
        }

        self.pending_identity_provider_forward_groups.insert(
            query_id.clone(),
            PendingIdentityProviderForwardGroup {
                query_id,
                identity,
                providers,
                channel,
                deadline: Instant::now() + IDENTITY_PROVIDER_QUERY_TIMEOUT,
                remaining_requests,
                limit,
            },
        );
    }

    fn respond_identity_providers(
        &mut self,
        channel: request_response::ResponseChannel<RelayExchangeResponse>,
        query_id: String,
        identity: IdentityId,
        providers: Vec<IdentityProviderCandidate>,
        limit: usize,
    ) {
        let providers = Self::dedupe_identity_provider_candidates(providers, limit);
        if let Err(e) = self.swarm.behaviour_mut().relay_exchange.send_response(
            channel,
            RelayExchangeResponse::IdentityProviders {
                query_id,
                identity,
                providers,
            },
        ) {
            warn!("Failed to send identity provider response: {e:?}");
        }
    }

    fn check_identity_provider_forward_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<_> = self
            .pending_identity_provider_forward_groups
            .iter()
            .filter_map(|(query_id, pending)| (pending.deadline <= now).then_some(query_id.clone()))
            .collect();

        for query_id in timed_out {
            if let Some(pending) = self
                .pending_identity_provider_forward_groups
                .remove(&query_id)
            {
                self.pending_identity_provider_forwards
                    .retain(|_, pending_query_id| pending_query_id != &query_id);
                self.respond_identity_providers(
                    pending.channel,
                    pending.query_id,
                    pending.identity,
                    pending.providers,
                    pending.limit,
                );
            }
        }
    }

    /// Publish a file to the content store. Returns the ContentId.
    pub fn publish_file(&mut self, file_path: &Path) -> Result<ContentId, NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
        self.publish_bytes(&data)
    }

    fn publish_bytes(&mut self, data: &[u8]) -> Result<ContentId, NetworkError> {
        let content_id = ContentId::from_bytes(&data);

        let signature = self.identity.sign(&data);

        let manifest = ContentManifest {
            content_id: content_id.clone(),
            size: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
            publisher_key: self.identity.public_key_bytes().to_vec(),
            signature,
        };

        self.store
            .publish(&data, &manifest)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        // Announce as DHT provider
        if let Err(e) = self.announce_provider(&content_id) {
            debug!("DHT announcement skipped: {e}");
        }

        Ok(content_id)
    }

    /// Publish a file and bind the resulting CID to an opaque path in this
    /// node's signed identity namespace.
    pub fn publish_file_at_path(
        &mut self,
        file_path: &Path,
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64), NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
        self.publish_bytes_at_path(&data, path)
    }

    fn publish_bytes_at_path(
        &mut self,
        data: &[u8],
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64), NetworkError> {
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_id = self.publish_bytes(data)?;
        let action = UpdateAction::SetPath {
            path: address.path().to_string(),
            content_id: content_id.clone(),
        };

        let entry = match self
            .update_logs
            .get(&identity)
            .and_then(|entries| entries.last())
        {
            Some(previous) => previous
                .append(action, |bytes| self.identity.sign(bytes))
                .map_err(|e| NetworkError::Protocol(e.to_string()))?,
            None => UpdateLogEntry::genesis(self.identity.public_key_bytes(), action, |bytes| {
                self.identity.sign(bytes)
            })
            .map_err(|e| NetworkError::Protocol(e.to_string()))?,
        };
        let latest_sequence = entry.body.sequence;
        let entries_to_save = {
            let entries = self.update_logs.entry(identity.clone()).or_default();
            entries.push(entry);
            entries.clone()
        };
        self.store
            .save_update_log(&identity, &entries_to_save)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        if let Err(e) = self.announce_update_log_provider(&identity) {
            debug!("Update-log DHT announcement skipped: {e}");
        }
        if let Err(e) = self.refresh_local_identity_head_hint(&identity) {
            debug!("Identity-head hint refresh skipped: {e}");
        }

        Ok((content_id, address, latest_sequence))
    }

    fn ensure_local_identity_encryption_key(
        &mut self,
    ) -> Result<IdentityEncryptionKey, NetworkError> {
        if self.local_encryption_key.is_none() {
            let now = unix_now();
            let (public_key, private_key) = generate_identity_encryption_keypair(
                self.identity.identity_id(),
                "enc_x25519_local_v0".to_string(),
                now,
            );
            self.store
                .save_local_identity_encryption_keypair(&LocalIdentityEncryptionKeypair {
                    public_key: public_key.clone(),
                    private_key: private_key.clone(),
                })
                .map_err(|e| NetworkError::Protocol(e.to_string()))?;
            self.local_encryption_key = Some((public_key, private_key));
        }

        let public_key = self
            .local_encryption_key
            .as_ref()
            .map(|(public_key, _)| public_key.clone())
            .expect("local encryption key is initialized");
        if !self.local_encryption_key_published {
            let now = unix_now();
            let record = IdentityEncryptionKeyRecord::new(
                self.identity.public_key_bytes(),
                self.identity.identity_id(),
                vec![public_key.clone()],
                now,
                now,
                |bytes| self.identity.sign(bytes),
            )
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
            let record_bytes =
                serde_json::to_vec(&record).map_err(|e| NetworkError::Protocol(e.to_string()))?;
            self.publish_bytes_at_path(&record_bytes, IDENTITY_ENCRYPTION_KEYS_PATH)?;
            self.local_encryption_key_published = true;
        }

        Ok(public_key)
    }

    fn load_persisted_local_encryption_key(
        store: &ContentStore,
        identity: &NodeIdentity,
    ) -> Result<Option<(IdentityEncryptionKey, IdentityEncryptionPrivateKey)>, NetworkError> {
        let Some(keypair) = store
            .load_local_identity_encryption_keypair()
            .map_err(|e| NetworkError::Protocol(e.to_string()))?
        else {
            return Ok(None);
        };

        if keypair.public_key.key_id != keypair.private_key.key_id {
            return Err(NetworkError::Protocol(
                "local identity encryption keypair has mismatched key ids".to_string(),
            ));
        }
        if keypair.private_key.identity != identity.identity_id() {
            return Err(NetworkError::Protocol(
                "local identity encryption private key belongs to a different identity".to_string(),
            ));
        }

        Ok(Some((keypair.public_key, keypair.private_key)))
    }

    fn encrypt_object(
        &self,
        plaintext: Vec<u8>,
        content_type: String,
        recipients: Vec<EncryptedObjectRecipient>,
    ) -> Result<crate::command::EncryptedObjectResponse, NetworkError> {
        let envelope = EncryptedObjectEnvelope::encrypt(
            self.identity.public_key_bytes(),
            self.identity.identity_id(),
            &plaintext,
            content_type,
            None,
            recipients,
            unix_now(),
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let recipient_count = envelope.body.recipients.len();
        let data = envelope
            .to_bytes()
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        Ok(crate::command::EncryptedObjectResponse {
            size: data.len() as u64,
            data,
            recipient_count,
        })
    }

    fn decrypt_object(
        &self,
        encrypted_object: &[u8],
    ) -> Result<crate::command::DecryptedObjectResponse, NetworkError> {
        let Some((_, private_key)) = self.local_encryption_key.as_ref() else {
            return Err(NetworkError::Protocol(
                "local identity encryption key is not available".to_string(),
            ));
        };
        let envelope = EncryptedObjectEnvelope::from_bytes(encrypted_object)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_type = envelope.body.plaintext.media_type.clone();
        let plaintext = decrypt_encrypted_object_for_recipient(&envelope, private_key)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        Ok(crate::command::DecryptedObjectResponse {
            size: plaintext.len() as u64,
            plaintext,
            content_type,
        })
    }

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
        Ok(record)
    }

    fn list_pending_ingress(&self) -> Vec<IngressRecord> {
        self.pending_ingress
            .iter()
            .filter(|record| record.status == IngressStatus::Pending)
            .cloned()
            .collect()
    }

    fn accept_ingress(&mut self, ingress_id: &str) -> Result<IngressRecord, NetworkError> {
        self.decide_ingress(ingress_id, IngressStatus::Accepted)
    }

    fn open_ingress(
        &self,
        ingress_id: &str,
    ) -> Result<crate::command::DecryptedObjectResponse, NetworkError> {
        let record = self
            .pending_ingress
            .iter()
            .find(|record| record.ingress_id == ingress_id)
            .ok_or_else(|| {
                NetworkError::InvalidInput(format!("ingress envelope not found: {ingress_id}"))
            })?;
        self.decrypt_object(&record.encrypted_object)
    }

    fn reject_ingress(&mut self, ingress_id: &str) -> Result<IngressRecord, NetworkError> {
        self.decide_ingress(ingress_id, IngressStatus::Rejected)
    }

    fn decide_ingress(
        &mut self,
        ingress_id: &str,
        status: IngressStatus,
    ) -> Result<IngressRecord, NetworkError> {
        let now = unix_now();
        let record = self
            .pending_ingress
            .iter_mut()
            .find(|record| record.ingress_id == ingress_id)
            .ok_or_else(|| {
                NetworkError::InvalidInput(format!("ingress envelope not found: {ingress_id}"))
            })?;
        if record.status != IngressStatus::Pending {
            return Err(NetworkError::InvalidInput(format!(
                "ingress envelope is not pending: {ingress_id}"
            )));
        }
        record.status = status;
        match record.status {
            IngressStatus::Accepted => record.accepted_at = Some(now),
            IngressStatus::Rejected => record.rejected_at = Some(now),
            IngressStatus::Pending => {}
        }
        Ok(record.clone())
    }

    fn publish_reachability(
        &mut self,
        sequence_hint: u64,
        expires_at: u64,
        live: Vec<LiveReachabilityEndpoint>,
        offline_ingress: Vec<OfflineIngressEndpoint>,
    ) -> Result<PublishReachabilityResponse, NetworkError> {
        let now = unix_now();
        let identity = self.identity.identity_id();
        let record = ReachabilityRecord::new(
            self.identity.public_key_bytes(),
            identity.clone(),
            sequence_hint,
            now,
            expires_at,
            live,
            offline_ingress,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let record_bytes =
            serde_json::to_vec(&record).map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let (content_id, address, latest_sequence) =
            self.publish_bytes_at_path(&record_bytes, SIGNED_REACHABILITY_PATH)?;
        let record = VerifiedReachability {
            identity: identity.clone(),
            sequence_hint,
            expires_at,
            live: record.body.live,
            offline_ingress: record.body.offline_ingress,
        };

        Ok(PublishReachabilityResponse {
            identity: identity.to_string(),
            path: SIGNED_REACHABILITY_PATH.to_string(),
            address: address.to_string(),
            latest_sequence,
            content_id: content_id.to_string(),
            record,
        })
    }

    fn publish_update_log_snapshot(
        &mut self,
        identity: &IdentityId,
    ) -> Result<Option<ContentId>, NetworkError> {
        let Some(entries) = self.update_logs.get(identity).cloned() else {
            return Ok(None);
        };
        verify_update_log_for_identity(identity, &entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        let data =
            serde_json::to_vec(&entries).map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let content_id = ContentId::from_bytes(&data);
        let signature = self.identity.sign(&data);
        let manifest = ContentManifest {
            content_id: content_id.clone(),
            size: data.len() as u64,
            content_type: "application/jolt-update-log+json".to_string(),
            publisher_key: self.identity.public_key_bytes().to_vec(),
            signature,
        };

        self.store
            .publish(&data, &manifest)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        if let Err(e) = self.announce_provider(&content_id) {
            debug!("Update-log snapshot DHT announcement skipped: {e}");
        }

        Ok(Some(content_id))
    }

    fn refresh_local_identity_head_hint(
        &mut self,
        identity: &IdentityId,
    ) -> Result<(), NetworkError> {
        if identity != &self.identity.identity_id() {
            return Ok(());
        }

        let Some(entries) = self.update_logs.get(identity) else {
            return Ok(());
        };
        let Some(latest) = entries.last() else {
            return Ok(());
        };

        let now = unix_now();
        let provider_addrs = self
            .swarm
            .listeners()
            .map(|addr| {
                if addr
                    .iter()
                    .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                {
                    addr.to_string()
                } else {
                    addr.clone()
                        .with(Protocol::P2p(*self.swarm.local_peer_id()))
                        .to_string()
                }
            })
            .collect::<Vec<_>>();
        if provider_addrs.is_empty() {
            return Ok(());
        }

        let hint = IdentityHeadHint::new(
            self.identity.public_key_bytes(),
            identity.clone(),
            self.swarm.local_peer_id().to_string(),
            provider_addrs,
            None,
            latest.body.sequence,
            latest.entry_hash(),
            now,
            now + RELAY_RECORD_TTL_SECS,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        self.record_identity_head_hint(hint)
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

    fn load_persisted_local_update_log(
        store: &ContentStore,
        identity: &NodeIdentity,
    ) -> Result<HashMap<IdentityId, Vec<UpdateLogEntry>>, NetworkError> {
        let identity_id = identity.identity_id();
        let Some(entries) = store.load_update_log(&identity_id).map_err(|e| {
            NetworkError::Protocol(format!(
                "failed to load persisted update log for {identity_id}: {e}"
            ))
        })?
        else {
            return Ok(HashMap::new());
        };

        verify_update_log_for_identity(&identity_id, &entries).map_err(|e| {
            NetworkError::Protocol(format!(
                "invalid persisted update log for {identity_id}: {e}"
            ))
        })?;

        let mut update_logs = HashMap::new();
        update_logs.insert(identity_id, entries);
        Ok(update_logs)
    }

    /// Store a verified update log for an identity, ignoring stale valid logs.
    pub fn store_verified_update_log(
        &mut self,
        identity: IdentityId,
        entries: Vec<UpdateLogEntry>,
    ) -> Result<(), NetworkError> {
        let candidate_sequence = verify_update_log_for_identity(&identity, &entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let current_sequence = self
            .update_logs
            .get(&identity)
            .and_then(|current| verify_update_log_for_identity(&identity, current).ok());

        if current_sequence
            .map(|current| candidate_sequence > current)
            .unwrap_or(true)
        {
            self.update_logs.insert(identity.clone(), entries);
            if let Err(e) = self.refresh_local_identity_head_hint(&identity) {
                debug!("Identity-head hint refresh skipped: {e}");
            }
        }

        Ok(())
    }

    /// Return the verified update log entries this node knows for an identity.
    pub fn update_log_entries(&self, identity: &IdentityId) -> Option<&[UpdateLogEntry]> {
        self.update_logs.get(identity).map(Vec::as_slice)
    }

    /// Resolve a Jolt address from this node's verified update-log cache.
    pub fn resolve_cached_jolt_address(
        &self,
        address: &JoltAddress,
        now: Option<u64>,
    ) -> Result<ResolvedJoltTarget, NetworkError> {
        let entries = self.update_logs.get(address.identity()).ok_or_else(|| {
            NetworkError::Protocol(format!(
                "No verified update log cached for {}",
                address.identity()
            ))
        })?;
        resolve_jolt_address(address, entries, now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))
    }

    fn resolve_response_from_cache(
        &self,
        address: &JoltAddress,
        now: Option<u64>,
        source: impl Into<String>,
    ) -> Result<ResolveResponse, NetworkError> {
        let entries = self.update_logs.get(address.identity()).ok_or_else(|| {
            NetworkError::Protocol(format!(
                "No verified update log cached for {}",
                address.identity()
            ))
        })?;
        let latest_sequence = verify_update_log_for_identity(address.identity(), entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let target = resolve_jolt_address(address, entries, now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        Ok(ResolveResponse {
            address: address.to_string(),
            identity: target.identity.to_string(),
            path: target.path,
            latest_sequence,
            content_id: target.content_id.to_string(),
            reachability_hints: target.reachability,
            source: source.into(),
        })
    }

    fn published_content_inventory(&self) -> Vec<PublishedContentInfo> {
        let identity = self.identity.identity_id();
        let current_paths = self.current_local_paths();

        let mut content_paths: HashMap<String, (String, u64)> = HashMap::new();
        for (path, (content_id, sequence)) in &current_paths {
            content_paths.insert(content_id.to_string(), (path.clone(), *sequence));
        }

        let pin_records = self.store.load_home_relay_pin_records().unwrap_or_default();

        self.store
            .list_published_content()
            .into_iter()
            .filter(|entry| entry.content_type != "application/jolt-update-log+json")
            .map(|entry| {
                let path = content_paths.get(&entry.content_id).cloned();
                let (path, local_sequence) = match path {
                    Some((path, sequence)) => (Some(path), Some(sequence)),
                    None => (None, None),
                };
                let address = path
                    .as_deref()
                    .and_then(|path| JoltAddress::new(identity.clone(), path).ok())
                    .map(|address| address.to_string());
                let pin_record =
                    Self::matching_pin_record(&pin_records, path.as_deref(), &entry.content_id);
                let pin_state = match (&pin_record, local_sequence) {
                    (Some(record), Some(sequence))
                        if record.content_id == entry.content_id
                            && record.latest_sequence >= sequence =>
                    {
                        "relay_backed"
                    }
                    (Some(_), Some(_)) => "needs_repin",
                    (Some(record), None) if record.content_id == entry.content_id => "relay_backed",
                    _ => "local_only",
                }
                .to_string();

                PublishedContentInfo {
                    content_id: entry.content_id,
                    size: entry.size,
                    path,
                    address,
                    local_sequence,
                    pin_state,
                    relay: pin_record.map(|record| PublishedRelayInfo {
                        peer_id: record.relay_peer_id.clone(),
                        multiaddr: record.relay_multiaddr.clone(),
                        api_url: record.relay_api_url.clone(),
                    }),
                    pinned_content_id: pin_record.map(|record| record.content_id.clone()),
                    pinned_sequence: pin_record.map(|record| record.latest_sequence),
                }
            })
            .collect()
    }

    fn connected_bootstrap_peer_count(&self) -> usize {
        self.swarm
            .connected_peers()
            .filter(|peer| self.bootstrap_peer_ids.contains(peer))
            .count()
    }

    fn bootstrap_state(&self, connected_bootstrap_peers: usize) -> String {
        if self.effective_bootstrap_relays.is_empty() {
            if self.swarm.connected_peers().next().is_some() {
                "connected"
            } else {
                "disconnected"
            }
        } else if connected_bootstrap_peers > 0 {
            "connected"
        } else if self.last_bootstrap_error.is_some() {
            "degraded"
        } else {
            "bootstrapping"
        }
        .to_string()
    }

    fn peer_hint_multiaddr(remote_addr: &str, peer_id: libp2p::PeerId) -> String {
        if remote_addr.contains("/p2p/") {
            return remote_addr.to_string();
        }

        match remote_addr.parse::<Multiaddr>() {
            Ok(addr) => addr.with(Protocol::P2p(peer_id)).to_string(),
            Err(_) => remote_addr.to_string(),
        }
    }

    fn current_local_paths(&self) -> HashMap<String, (ContentId, u64)> {
        let identity = self.identity.identity_id();
        let mut current_paths: HashMap<String, (ContentId, u64)> = HashMap::new();
        if let Some(entries) = self.update_logs.get(&identity) {
            for entry in entries {
                match &entry.body.action {
                    UpdateAction::SetPath { path, content_id } => {
                        current_paths
                            .insert(path.clone(), (content_id.clone(), entry.body.sequence));
                    }
                    UpdateAction::RemovePath { path } => {
                        current_paths.remove(path);
                    }
                    _ => {}
                }
            }
        }
        current_paths
    }

    fn matching_pin_record<'a>(
        records: &'a [HomeRelayPinRecord],
        path: Option<&str>,
        content_id: &str,
    ) -> Option<&'a HomeRelayPinRecord> {
        records
            .into_iter()
            .filter(|record| match (path, record.path.as_deref()) {
                (Some(path), Some(record_path)) => path == record_path,
                (None, _) => record.content_id == content_id,
                _ => false,
            })
            .max_by_key(|record| (record.latest_sequence, record.pinned_at))
    }

    fn record_home_relay_pin(
        &self,
        content_id: &str,
        requested_path: Option<String>,
        relay: HomeRelayConfig,
        latest_sequence: u64,
    ) -> Result<(), NetworkError> {
        let identity = self.identity.identity_id();
        let current_paths = self.current_local_paths();
        let paths = if let Some(path) = requested_path {
            match current_paths.get(&path) {
                Some((path_content_id, _)) if path_content_id.to_string() == content_id => {
                    vec![Some(path)]
                }
                Some(_) => {
                    return Err(NetworkError::InvalidInput(format!(
                        "path {path} does not point at content {content_id}"
                    )));
                }
                None => {
                    return Err(NetworkError::InvalidInput(format!(
                        "path {path} is not locally published"
                    )));
                }
            }
        } else {
            let mut paths = current_paths
                .iter()
                .filter_map(|(path, (path_content_id, _))| {
                    if path_content_id.to_string() == content_id {
                        Some(Some(path.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if paths.is_empty() {
                paths.push(None);
            }
            paths
        };

        for path in paths {
            let address = path
                .as_deref()
                .and_then(|path| JoltAddress::new(identity.clone(), path).ok())
                .map(|address| address.to_string());
            let record = HomeRelayPinRecord {
                content_id: content_id.to_string(),
                path,
                address,
                relay_peer_id: relay.peer_id.clone(),
                relay_multiaddr: relay.multiaddr.clone(),
                relay_api_url: relay.api_url.clone(),
                latest_sequence,
                pinned_at: unix_now(),
            };
            self.store
                .save_home_relay_pin_record(record)
                .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        }

        Ok(())
    }

    fn request_daemon_resolve_from_provider(
        &mut self,
        address: JoltAddress,
        now: Option<u64>,
        provider: &libp2p::PeerId,
        response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
        fallback_response: Option<ResolveResponse>,
    ) {
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
            .send_request(provider, request);
        self.pending_daemon_resolutions.insert(
            request_id,
            PendingDaemonResolve {
                address,
                now,
                provider: *provider,
                response_tx,
                deadline: Instant::now() + self.resolve_timeout,
                fallback_response,
            },
        );
    }

    fn should_refresh_cached_resolution(&self, identity: &IdentityId) -> bool {
        let key = Self::update_log_provider_key(identity);
        self.discovered_providers
            .get(&key)
            .is_some_and(|providers| !providers.is_empty())
            || self.swarm.connected_peers().next().is_some()
            || !self.effective_bootstrap_relays.is_empty()
    }

    fn request_pending_resolves_from_provider(
        &mut self,
        identity: &IdentityId,
        provider: &libp2p::PeerId,
    ) {
        let Some(pending) = self.pending_resolves.remove(identity) else {
            return;
        };

        for pending in pending {
            self.request_daemon_resolve_from_provider(
                pending.address,
                None,
                provider,
                pending.response_tx,
                pending.fallback_response,
            );
        }
    }

    fn check_resolve_timeouts(&mut self) {
        let now = Instant::now();
        let mut empty = Vec::new();
        let no_bootstrap_relays = self.effective_bootstrap_relays.is_empty()
            && self.swarm.connected_peers().next().is_none();
        let relay_unreachable = !self.effective_bootstrap_relays.is_empty()
            && self.connected_bootstrap_peer_count() == 0;

        for (identity, pending) in &mut self.pending_resolves {
            let mut still_waiting = Vec::new();
            for pending in pending.drain(..) {
                if pending.deadline <= now {
                    let result = pending.fallback_response.map(Ok).unwrap_or_else(|| {
                        Err(Self::identity_provider_failure(
                            identity,
                            no_bootstrap_relays,
                            relay_unreachable,
                        ))
                    });
                    let _ = pending.response_tx.send(result);
                } else {
                    still_waiting.push(pending);
                }
            }
            *pending = still_waiting;
            if pending.is_empty() {
                empty.push(identity.clone());
            }
        }

        for identity in empty {
            self.pending_resolves.remove(&identity);
        }

        let timed_out: Vec<_> = self
            .pending_daemon_resolutions
            .iter()
            .filter_map(|(request_id, pending)| (pending.deadline <= now).then_some(*request_id))
            .collect();

        for request_id in timed_out {
            if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                let result = pending
                    .fallback_response
                    .map(Ok)
                    .unwrap_or(Err(NetworkError::Timeout));
                let _ = pending.response_tx.send(result);
            }
        }
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

    /// Add bootstrap peers to Kademlia, dial them, and initiate DHT bootstrap.
    pub fn bootstrap_dht(&mut self, bootstrap_addrs: &[Multiaddr]) -> Result<(), NetworkError> {
        for addr in bootstrap_addrs {
            let (peer_id, transport) = crate::bootstrap::parse_bootstrap_addr(&addr.to_string())?;
            self.bootstrap_peer_ids.insert(peer_id);
            // Add address to Kademlia routing table
            // For iroh transport, use the /p2p/<peer_id> addr; for TCP, use the transport addr
            let kad_addr = if transport.iter().count() == 0 {
                addr.clone() // pure /p2p/ addr (iroh)
            } else {
                transport.clone()
            };
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, kad_addr.clone());

            // Dial the full multiaddr so iroh can extract the NodeId
            if let Err(e) = self.swarm.dial(addr.clone()) {
                let message = format!("Failed to dial bootstrap peer {peer_id}: {e}");
                warn!("{message}");
                self.last_bootstrap_error = Some(message);
                let _ = self
                    .store
                    .mark_discovered_peer_hint_failure(&addr.to_string());
            }
            info!("Added bootstrap peer: {peer_id}");
        }

        match self.swarm.behaviour_mut().kademlia.bootstrap() {
            Ok(_) => {}
            Err(e) => {
                let message = format!("Bootstrap failed: {e:?}");
                self.last_bootstrap_error = Some(message.clone());
                return Err(NetworkError::Dht(message));
            }
        }

        info!("DHT bootstrap initiated");
        Ok(())
    }

    /// Dial a peer by multiaddr. A `/p2p/<peer-id>` suffix is recommended so the
    /// address can be registered with Kademlia before dialing.
    pub fn connect_peer_multiaddr(
        &mut self,
        addr: &str,
    ) -> Result<Option<libp2p::PeerId>, NetworkError> {
        if addr.contains("/p2p/") {
            let (peer_id, transport_addr) = crate::bootstrap::parse_bootstrap_addr(addr)?;
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, transport_addr.clone());
            self.swarm.add_peer_address(peer_id, transport_addr.clone());
            self.swarm
                .dial(transport_addr)
                .map_err(|e| NetworkError::Swarm(format!("Dial failed: {e}")))?;
            Ok(Some(peer_id))
        } else {
            let multiaddr: Multiaddr = addr
                .parse()
                .map_err(|e| NetworkError::Swarm(format!("Invalid peer multiaddr: {e}")))?;
            self.swarm
                .dial(multiaddr)
                .map_err(|e| NetworkError::Swarm(format!("Dial failed: {e}")))?;
            Ok(None)
        }
    }

    /// Announce this node as a provider for the given content in the DHT.
    pub fn announce_provider(&mut self, content_id: &ContentId) -> Result<(), NetworkError> {
        let key = libp2p::kad::RecordKey::new(&content_id.to_string().into_bytes());
        self.swarm
            .behaviour_mut()
            .kademlia
            .start_providing(key)
            .map_err(|e| NetworkError::Dht(format!("Failed to announce provider: {e:?}")))?;
        info!("Announcing as DHT provider for: {content_id}");
        Ok(())
    }

    /// Deterministic DHT provider key for peers that may serve an identity's update log.
    pub fn update_log_provider_key(identity: &IdentityId) -> String {
        format!("jolt:update-log:{identity}")
    }

    /// Announce this node as a provider for an identity's update log in the DHT.
    pub fn announce_update_log_provider(
        &mut self,
        identity: &IdentityId,
    ) -> Result<(), NetworkError> {
        let key_str = Self::update_log_provider_key(identity);
        let key = libp2p::kad::RecordKey::new(&key_str.clone().into_bytes());
        self.swarm
            .behaviour_mut()
            .kademlia
            .start_providing(key)
            .map_err(|e| {
                NetworkError::Dht(format!("Failed to announce update-log provider: {e:?}"))
            })?;
        info!("Announcing as update-log provider for: {identity}");
        Ok(())
    }

    /// Take the first discovered provider for the given content.
    pub fn take_discovered_provider(&mut self, content_id: &ContentId) -> Option<libp2p::PeerId> {
        let key = content_id.to_string();
        if let Some(providers) = self.discovered_providers.get_mut(&key) {
            if !providers.is_empty() {
                return Some(providers.remove(0));
            }
        }
        None
    }

    /// Take the first discovered provider for the given identity's update log.
    pub fn take_discovered_update_log_provider(
        &mut self,
        identity: &IdentityId,
    ) -> Option<libp2p::PeerId> {
        self.take_discovered_update_log_provider_except(identity, None)
    }

    fn take_discovered_update_log_provider_except(
        &mut self,
        identity: &IdentityId,
        excluded: Option<&libp2p::PeerId>,
    ) -> Option<libp2p::PeerId> {
        let key = Self::update_log_provider_key(identity);
        if let Some(providers) = self.discovered_providers.get_mut(&key) {
            if let Some(excluded) = excluded {
                providers.retain(|provider| provider != excluded);
            }
            if let Some(position) = providers
                .iter()
                .position(|provider| Some(provider) != excluded)
            {
                return Some(providers.remove(position));
            }
        }
        None
    }

    /// Query the DHT for providers of the given content.
    pub fn find_providers(&mut self, content_id: &ContentId) -> libp2p::kad::QueryId {
        let key = libp2p::kad::RecordKey::new(&content_id.to_string().into_bytes());
        self.swarm.behaviour_mut().kademlia.get_providers(key)
    }

    /// Query the DHT for providers of an identity's update log.
    pub fn find_update_log_providers(&mut self, identity: &IdentityId) -> libp2p::kad::QueryId {
        let key =
            libp2p::kad::RecordKey::new(&Self::update_log_provider_key(identity).into_bytes());
        let query_id = self.swarm.behaviour_mut().kademlia.get_providers(key);
        self.start_identity_provider_relay_query(identity);
        query_id
    }

    /// Handle a single swarm event.
    pub fn handle_swarm_event(&mut self, event: SwarmEvent<JoltBehaviourEvent>) {
        match event {
            // --- mDNS ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(
                peers,
            ))) => {
                for (peer_id, addr) in peers {
                    info!("mDNS discovered peer: {peer_id} at {addr}");
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    if let Err(e) = self.swarm.dial(addr) {
                        debug!("Failed to dial discovered peer: {e}");
                    }
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::Mdns(libp2p::mdns::Event::Expired(
                peers,
            ))) => {
                for (peer_id, _addr) in peers {
                    debug!("mDNS peer expired: {peer_id}");
                }
            }

            // --- Content Fetch ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                info!("Received content request for: {}", request.content_id);

                let response =
                    if let Some(content_data) = self.store.get_content(&request.content_id) {
                        ContentResponse {
                            data: content_data.data,
                            signature: content_data.signature,
                            publisher_key: content_data.publisher_key,
                        }
                    } else {
                        debug!("Content not found: {}", request.content_id);
                        ContentResponse {
                            data: vec![],
                            signature: vec![],
                            publisher_key: vec![],
                        }
                    };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .content_fetch
                    .send_response(channel, response)
                {
                    warn!("Failed to send response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                info!("Received content response ({} bytes)", response.data.len());

                // Try fetch manager first (daemon-managed fetches)
                if !response.data.is_empty() {
                    if let Some(content_id_str) = self
                        .fetch_manager
                        .on_content_response(request_id, response.clone())
                    {
                        // Cache the fetched content for re-sharing
                        if let Err(e) = self.store.cache_content(
                            &content_id_str,
                            &response.data,
                            &response.publisher_key,
                            &response.signature,
                        ) {
                            warn!("Failed to cache content: {e}");
                        } else {
                            info!("Content cached for re-sharing: {content_id_str}");
                        }
                        return;
                    }
                }

                // Fall back to legacy pending_fetches (for non-daemon operations)
                if let Some((content_id_str, tx)) = self.pending_fetches.remove(&request_id) {
                    if !response.data.is_empty() {
                        if let Err(e) = self.store.cache_content(
                            &content_id_str,
                            &response.data,
                            &response.publisher_key,
                            &response.signature,
                        ) {
                            warn!("Failed to cache content: {e}");
                        }
                    }
                    let _ = tx.send(Ok(response));
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::ContentFetch(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound request failed: {error}");

                // Try fetch manager first
                if self.fetch_manager.on_request_failure(request_id) {
                    return;
                }

                // Fall back to legacy pending_fetches
                if let Some((_content_id, tx)) = self.pending_fetches.remove(&request_id) {
                    let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                }
            }

            // --- Update Log Sync ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                info!("Received update-log request for: {}", request.identity);

                let response = UpdateLogResponse {
                    entries: self
                        .update_logs
                        .get(&request.identity)
                        .cloned()
                        .unwrap_or_default(),
                };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .update_log_sync
                    .send_response(channel, response)
                {
                    warn!("Failed to send update-log response: {e:?}");
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::Message {
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => {
                info!(
                    "Received update-log response ({} entries)",
                    response.entries.len()
                );

                if let Some((identity, tx)) = self.pending_update_log_requests.remove(&request_id) {
                    let result = if response.entries.is_empty() {
                        Ok(response)
                    } else {
                        self.store_verified_update_log(identity, response.entries.clone())
                            .map(|_| response)
                    };
                    let _ = tx.send(result);
                } else if let Some(pending) = self.pending_update_log_pins.remove(&request_id) {
                    let result = if response.entries.is_empty() {
                        Err(NetworkError::Protocol(format!(
                            "No update log entries returned for {}",
                            pending.identity
                        )))
                    } else {
                        verify_update_log_for_identity(&pending.identity, &response.entries)
                            .map_err(|e| NetworkError::Protocol(e.to_string()))
                            .and_then(|sequence| {
                                self.store_verified_update_log(
                                    pending.identity.clone(),
                                    response.entries,
                                )?;
                                self.announce_update_log_provider(&pending.identity)?;
                                Ok(sequence)
                            })
                    };
                    let _ = pending.response_tx.send(result);
                } else if let Some((address, now, _provider, tx)) =
                    self.pending_jolt_resolutions.remove(&request_id)
                {
                    let result = if response.entries.is_empty() {
                        Err(Self::identity_head_invalid_failure(
                            address.identity(),
                            "provider returned no update log entries",
                        ))
                    } else {
                        self.store_verified_update_log(address.identity().clone(), response.entries)
                            .map_err(|e| {
                                Self::identity_head_invalid_failure(
                                    address.identity(),
                                    e.to_string(),
                                )
                            })
                            .and_then(|_| self.resolve_cached_jolt_address(&address, now))
                    };
                    let _ = tx.send(result);
                } else if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                    let result = if response.entries.is_empty() {
                        pending.fallback_response.map(Ok).unwrap_or_else(|| {
                            Err(Self::identity_head_invalid_failure(
                                pending.address.identity(),
                                "provider returned no update log entries",
                            ))
                        })
                    } else {
                        self.store_verified_update_log(
                            pending.address.identity().clone(),
                            response.entries,
                        )
                        .map_err(|e| {
                            Self::identity_head_invalid_failure(
                                pending.address.identity(),
                                e.to_string(),
                            )
                        })
                        .and_then(|_| {
                            self.resolve_response_from_cache(
                                &pending.address,
                                pending.now,
                                "network",
                            )
                        })
                    };
                    let _ = pending.response_tx.send(result);
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound update-log request failed: {error}");

                if let Some((_identity, tx)) = self.pending_update_log_requests.remove(&request_id)
                {
                    let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                }
                if let Some(pending) = self.pending_update_log_pins.remove(&request_id) {
                    let _ = pending
                        .response_tx
                        .send(Err(NetworkError::Protocol(error.to_string())));
                }
                if let Some((address, now, provider, tx)) =
                    self.pending_jolt_resolutions.remove(&request_id)
                {
                    if let Some(next_provider) = self.take_discovered_update_log_provider_except(
                        address.identity(),
                        Some(&provider),
                    ) {
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
                            .send_request(&next_provider, request);
                        self.pending_jolt_resolutions
                            .insert(request_id, (address, now, next_provider, tx));
                    } else {
                        let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                    }
                }
                if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                    if let Some(next_provider) = self.take_discovered_update_log_provider_except(
                        pending.address.identity(),
                        Some(&pending.provider),
                    ) {
                        self.request_daemon_resolve_from_provider(
                            pending.address,
                            pending.now,
                            &next_provider,
                            pending.response_tx,
                            pending.fallback_response,
                        );
                    } else {
                        let result = pending
                            .fallback_response
                            .map(Ok)
                            .unwrap_or_else(|| Err(NetworkError::Protocol(error.to_string())));
                        let _ = pending.response_tx.send(result);
                    }
                }
            }

            // --- Relay Record Exchange ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Request {
                            request, channel, ..
                        },
                    ..
                },
            )) => {
                let now = unix_now();
                let response = match request {
                    RelayExchangeRequest::GetRelays {
                        limit,
                        capabilities,
                    } => RelayExchangeResponse::Relays {
                        records: self.relay_records_for_exchange(
                            limit as usize,
                            &capabilities,
                            now,
                        ),
                    },
                    RelayExchangeRequest::AnnounceRelays { records } => {
                        let (accepted, rejected) = self.store_relay_records(records, now);
                        RelayExchangeResponse::Announced { accepted, rejected }
                    }
                    RelayExchangeRequest::GetIdentityHeads { limit } => {
                        RelayExchangeResponse::IdentityHeads {
                            hints: self.identity_head_hints_for_exchange(now, usize::from(limit)),
                        }
                    }
                    RelayExchangeRequest::AnnounceIdentityHeads { hints } => {
                        let (accepted, rejected) = self.record_identity_head_hints(hints, now);
                        RelayExchangeResponse::Announced { accepted, rejected }
                    }
                    RelayExchangeRequest::FindIdentityProviders {
                        query_id,
                        identity,
                        limit,
                        ttl,
                        deadline_unix_ms,
                    } => {
                        self.handle_identity_provider_query(
                            peer,
                            query_id,
                            identity,
                            limit,
                            ttl,
                            deadline_unix_ms,
                            channel,
                        );
                        return;
                    }
                };

                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .relay_exchange
                    .send_response(channel, response)
                {
                    warn!("Failed to send relay exchange response: {e:?}");
                } else {
                    self.mark_relay_peer_success(&peer, now);
                }
            }

            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::Message {
                    peer,
                    message:
                        request_response::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                },
            )) => match response {
                RelayExchangeResponse::Relays { records } => {
                    let now = unix_now();
                    let (accepted, rejected) = self.store_relay_records(records, now);
                    self.mark_relay_peer_success(&peer, now);
                    debug!(
                        "Stored relay exchange response records: {accepted} accepted, {rejected} rejected"
                    );
                }
                RelayExchangeResponse::Announced { accepted, rejected } => {
                    self.mark_relay_peer_success(&peer, unix_now());
                    debug!("Relay announcement response: {accepted} accepted, {rejected} rejected");
                }
                RelayExchangeResponse::IdentityHeads { hints } => {
                    let now = unix_now();
                    let (accepted, rejected) = self.record_identity_head_hints(hints, now);
                    self.mark_relay_peer_success(&peer, now);
                    debug!(
                        "Stored identity-head gossip response hints: {accepted} accepted, {rejected} rejected"
                    );
                }
                RelayExchangeResponse::IdentityProviders {
                    query_id: _,
                    identity,
                    providers,
                } => {
                    if self.handle_identity_diagnosis_response(request_id, providers.clone()) {
                        return;
                    }

                    if let Some(query_id) =
                        self.pending_identity_provider_forwards.remove(&request_id)
                    {
                        let mut should_respond = false;
                        if let Some(pending) = self
                            .pending_identity_provider_forward_groups
                            .get_mut(&query_id)
                        {
                            debug!(
                                "Received {} forwarded identity provider candidates for {}",
                                providers.len(),
                                identity
                            );
                            pending.providers.extend(providers);
                            pending.remaining_requests =
                                pending.remaining_requests.saturating_sub(1);
                            should_respond = pending.remaining_requests == 0
                                || pending.providers.len() >= pending.limit;
                        }

                        if should_respond {
                            if let Some(pending) = self
                                .pending_identity_provider_forward_groups
                                .remove(&query_id)
                            {
                                self.pending_identity_provider_forwards
                                    .retain(|_, pending_query_id| pending_query_id != &query_id);
                                self.respond_identity_providers(
                                    pending.channel,
                                    pending.query_id,
                                    pending.identity,
                                    pending.providers,
                                    pending.limit,
                                );
                            }
                        }
                    } else {
                        debug!(
                            "Received {} identity provider candidates for {}",
                            providers.len(),
                            identity
                        );
                        let providers =
                            self.record_identity_provider_candidates(&identity, providers);
                        if let Some(provider) = providers.first() {
                            self.request_pending_resolves_from_provider(&identity, provider);
                        }
                    }
                }
            },

            SwarmEvent::Behaviour(JoltBehaviourEvent::RelayExchange(
                request_response::Event::OutboundFailure {
                    peer,
                    request_id,
                    error,
                    ..
                },
            )) => {
                if self.handle_identity_diagnosis_failure(request_id, error.to_string()) {
                    self.mark_relay_peer_failure(&peer);
                    debug!("Outbound relay exchange failed: {error}");
                    return;
                }

                if let Some(query_id) = self.pending_identity_provider_forwards.remove(&request_id)
                {
                    let mut should_respond = false;
                    if let Some(pending) = self
                        .pending_identity_provider_forward_groups
                        .get_mut(&query_id)
                    {
                        pending.remaining_requests = pending.remaining_requests.saturating_sub(1);
                        should_respond = pending.remaining_requests == 0;
                    }

                    if should_respond {
                        if let Some(pending) = self
                            .pending_identity_provider_forward_groups
                            .remove(&query_id)
                        {
                            self.pending_identity_provider_forwards
                                .retain(|_, pending_query_id| pending_query_id != &query_id);
                            self.respond_identity_providers(
                                pending.channel,
                                pending.query_id,
                                pending.identity,
                                pending.providers,
                                pending.limit,
                            );
                        }
                    }
                }
                self.mark_relay_peer_failure(&peer);
                debug!("Outbound relay exchange failed: {error}");
            }

            // --- Identify ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => {
                debug!(
                    "Identified peer {peer_id}: {} protocols",
                    info.protocols.len()
                );
                for addr in &info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                }
                self.swarm.add_external_address(info.observed_addr.clone());
            }

            // --- Kademlia ---
            SwarmEvent::Behaviour(JoltBehaviourEvent::Kademlia(event)) => {
                match event {
                    libp2p::kad::Event::OutboundQueryProgressed { result, .. } => {
                        match result {
                            libp2p::kad::QueryResult::Bootstrap(Ok(ok)) => {
                                info!("DHT bootstrap step: {} remaining", ok.num_remaining);
                            }
                            libp2p::kad::QueryResult::Bootstrap(Err(e)) => {
                                let message = format!("DHT bootstrap error: {e:?}");
                                warn!("{message}");
                                self.last_bootstrap_error = Some(message);
                            }
                            libp2p::kad::QueryResult::StartProviding(Ok(_)) => {
                                info!("DHT provider announcement confirmed");
                            }
                            libp2p::kad::QueryResult::StartProviding(Err(e)) => {
                                warn!("DHT provider announcement FAILED: {e:?}");
                            }
                            libp2p::kad::QueryResult::GetProviders(Ok(ok)) => {
                                match ok {
                                    libp2p::kad::GetProvidersOk::FoundProviders {
                                        key, providers, ..
                                    } => {
                                        let key_str = String::from_utf8_lossy(key.as_ref()).to_string();
                                        let pending_identity = self
                                            .pending_resolves
                                            .keys()
                                            .find(|identity| {
                                                Self::update_log_provider_key(identity) == key_str
                                            })
                                            .cloned();
                                        for provider in providers {
                                            if provider != *self.swarm.local_peer_id() {
                                                info!("DHT found provider {provider} for {key_str}");
                                                self.discovered_providers
                                                    .entry(key_str.clone())
                                                    .or_default()
                                                    .push(provider);

                                                // If fetch manager is waiting for a provider, record it
                                                if self.fetch_manager.is_awaiting_provider(&key_str) {
                                                    let already_connected = self.swarm.connected_peers()
                                                        .any(|p| *p == provider);
                                                    self.fetch_manager
                                                        .on_provider_discovered(&key_str, provider, already_connected);
                                                }

                                                // Dial the provider (iroh handles NAT traversal automatically)
                                                if let Err(e) = self.swarm.dial(provider) {
                                                    debug!("Failed to dial provider {provider}: {e}");
                                                }

                                                if let Some(identity) = &pending_identity {
                                                    self.request_pending_resolves_from_provider(
                                                        identity,
                                                        &provider,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord {
                                        ..
                                    } => {
                                        info!("DHT provider search finished (no more records)");
                                    }
                                }
                            }
                            libp2p::kad::QueryResult::GetProviders(Err(e)) => {
                                warn!("DHT get_providers error: {e:?}");
                            }
                            _ => {}
                        }
                    }
                    libp2p::kad::Event::RoutingUpdated { peer, .. } => {
                        debug!("Kademlia routing updated: {peer}");
                    }
                    _ => {}
                }
            }

            // --- Swarm-level events ---
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address}");
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                let conn_info = PeerConnectionInfo::from_endpoint(&endpoint);
                info!(
                    "Connected to peer: {peer_id} (relayed: {}, transport: {})",
                    conn_info.is_relayed, conn_info.transport
                );
                let remote_addr = conn_info.remote_addr.clone();

                // Track connection quality (keep best: direct > relayed)
                let dominated = self
                    .peer_connections
                    .get(&peer_id)
                    .map(|existing| existing.is_relayed && !conn_info.is_relayed)
                    .unwrap_or(true);
                if dominated {
                    self.peer_connections.insert(peer_id, conn_info);
                }
                let should_exchange_relay_records = self.bootstrap_peer_ids.contains(&peer_id)
                    || self.relay_mesh_peer_ids.contains(&peer_id);
                if should_exchange_relay_records {
                    self.last_bootstrap_error = None;
                    self.start_relay_record_exchange(&peer_id);
                }
                let source = if self.bootstrap_peer_ids.contains(&peer_id) {
                    "bootstrap"
                } else if self.relay_mesh_peer_ids.contains(&peer_id) {
                    "relay_mesh"
                } else {
                    "connection"
                };
                let hint = Self::peer_hint_multiaddr(&remote_addr, peer_id);
                if let Err(e) = self.store.record_discovered_peer_hint(&hint, source) {
                    debug!("Failed to record discovered peer hint {hint}: {e}");
                }

                // Notify fetch manager in case this is a provider we're waiting for
                self.fetch_manager.on_peer_connected(&peer_id);
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                debug!("Disconnected from peer: {peer_id}");
                if !self.swarm.is_connected(&peer_id) {
                    self.peer_connections.remove(&peer_id);
                }
            }

            SwarmEvent::ExternalAddrConfirmed { address } => {
                info!("External address confirmed: {address}");
            }

            SwarmEvent::NewExternalAddrCandidate { address } => {
                info!("New external address candidate: {address}");
            }

            _ => {}
        }
    }

    /// Run the daemon event loop, processing both swarm events and incoming commands.
    pub async fn run_daemon_loop(&mut self, mut cmd_rx: mpsc::Receiver<DaemonCommand>) {
        let mut timeout_interval = tokio::time::interval(Duration::from_secs(1));
        let mut relay_mesh_interval = tokio::time::interval(RELAY_MESH_EXPLORATION_INTERVAL);

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event);
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(command) => {
                            let should_shutdown = matches!(command, DaemonCommand::Shutdown { .. });
                            self.handle_command(command);
                            if should_shutdown {
                                info!("Daemon shutting down");
                                return;
                            }
                        }
                        None => {
                            info!("Command channel closed, shutting down");
                            return;
                        }
                    }
                }
                _ = timeout_interval.tick() => {
                    self.fetch_manager.check_timeouts();
                    self.check_resolve_timeouts();
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
                }
                _ = relay_mesh_interval.tick() => {
                    self.check_identity_provider_forward_timeouts();
                    self.check_identity_diagnosis_timeouts();
                    self.explore_relay_mesh();
                }
            }
        }
    }

    /// Handle a single daemon command.
    fn handle_command(&mut self, command: DaemonCommand) {
        match command {
            DaemonCommand::Publish {
                file_path,
                path,
                response_tx,
            } => {
                let size = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
                let result = match path {
                    Some(path) => self.publish_file_at_path(&file_path, &path).map(
                        |(content_id, address, latest_sequence)| PublishResponse {
                            content_id: content_id.to_string(),
                            size,
                            path: Some(address.path().to_string()),
                            address: Some(address.to_string()),
                            latest_sequence: Some(latest_sequence),
                        },
                    ),
                    None => self
                        .publish_file(&file_path)
                        .map(|content_id| PublishResponse {
                            content_id: content_id.to_string(),
                            size,
                            path: None,
                            address: None,
                            latest_sequence: None,
                        }),
                };
                let _ = response_tx.send(result);
            }
            DaemonCommand::Fetch {
                content_id,
                response_tx,
            } => {
                // Check local store first
                if let Some(content_data) = self.store.get_content(&content_id) {
                    let _ = response_tx.send(Ok(FetchResult {
                        data: content_data.data.clone(),
                        content_id: content_id.clone(),
                        size: content_data.data.len() as u64,
                    }));
                    return;
                }

                // Try connected peers first
                let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
                info!(
                    "Fetch {content_id}: {} connected peers, querying them first",
                    peers.len()
                );
                let mut request_ids = Vec::new();
                let request = ContentRequest {
                    content_id: content_id.clone(),
                };
                for peer in &peers {
                    let req_id = self
                        .swarm
                        .behaviour_mut()
                        .content_fetch
                        .send_request(peer, request.clone());
                    request_ids.push(req_id);
                }

                // Also start DHT provider query
                info!("Fetch {content_id}: starting DHT provider query");
                let key = libp2p::kad::RecordKey::new(&content_id.clone().into_bytes());
                self.swarm.behaviour_mut().kademlia.get_providers(key);

                self.fetch_manager
                    .start_fetch(content_id, response_tx, request_ids);
            }
            DaemonCommand::Resolve {
                address,
                response_tx,
            } => {
                let address = match JoltAddress::from_str(&address) {
                    Ok(address) => address,
                    Err(e) => {
                        let _ = response_tx.send(Err(NetworkError::InvalidInput(e.to_string())));
                        return;
                    }
                };

                let fallback_response = self
                    .resolve_response_from_cache(&address, None, "cache")
                    .ok();
                let identity = address.identity().clone();

                if fallback_response.is_some() && !self.should_refresh_cached_resolution(&identity)
                {
                    let _ = response_tx.send(Ok(fallback_response.unwrap()));
                    return;
                }

                self.find_update_log_providers(&identity);

                if let Some(provider) = self.take_discovered_update_log_provider(&identity) {
                    self.request_daemon_resolve_from_provider(
                        address,
                        None,
                        &provider,
                        response_tx,
                        fallback_response,
                    );
                } else {
                    self.pending_resolves
                        .entry(identity)
                        .or_default()
                        .push(PendingResolve {
                            address,
                            response_tx,
                            deadline: Instant::now() + self.resolve_timeout,
                            fallback_response,
                        });
                }
            }
            DaemonCommand::DiagnoseIdentity {
                identity,
                response_tx,
            } => {
                self.diagnose_identity(identity, response_tx);
            }
            DaemonCommand::EnsureLocalIdentityEncryptionKey { response_tx } => {
                let result = self.ensure_local_identity_encryption_key();
                let _ = response_tx.send(result);
            }
            DaemonCommand::EncryptObject {
                plaintext,
                content_type,
                recipients,
                response_tx,
            } => {
                let result = self.encrypt_object(plaintext, content_type, recipients);
                let _ = response_tx.send(result);
            }
            DaemonCommand::DecryptObject {
                encrypted_object,
                response_tx,
            } => {
                let result = self.decrypt_object(&encrypted_object);
                let _ = response_tx.send(result);
            }
            DaemonCommand::PublishReachability {
                sequence_hint,
                expires_at,
                live,
                offline_ingress,
                response_tx,
            } => {
                let result =
                    self.publish_reachability(sequence_hint, expires_at, live, offline_ingress);
                let _ = response_tx.send(result);
            }
            DaemonCommand::SubmitIngress {
                receiver_id,
                encrypted_object,
                expires_at,
                response_tx,
            } => {
                let result = self.submit_ingress(receiver_id, encrypted_object, expires_at);
                let _ = response_tx.send(result);
            }
            DaemonCommand::ListPendingIngress { response_tx } => {
                let _ = response_tx.send(Ok(self.list_pending_ingress()));
            }
            DaemonCommand::OpenIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.open_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::AcceptIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.accept_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::RejectIngress {
                ingress_id,
                response_tx,
            } => {
                let result = self.reject_ingress(&ingress_id);
                let _ = response_tx.send(result);
            }
            DaemonCommand::ConnectPeer {
                multiaddr,
                response_tx,
            } => {
                let result =
                    self.connect_peer_multiaddr(&multiaddr)
                        .map(|peer_id| PeerConnectResponse {
                            peer_id: peer_id.map(|p| p.to_string()),
                            multiaddr,
                        });
                let _ = response_tx.send(result);
            }
            DaemonCommand::GetStatus { response_tx } => {
                let direct = self
                    .peer_connections
                    .values()
                    .filter(|c| !c.is_relayed)
                    .count();
                let relayed = self
                    .peer_connections
                    .values()
                    .filter(|c| c.is_relayed)
                    .count();
                let connected_bootstrap_peers = self.connected_bootstrap_peer_count();
                let now = unix_now();
                let known_relay_count = self.store.known_relay_count(now).unwrap_or_else(|e| {
                    warn!("Failed to load known relay count: {e}");
                    0
                });
                let relay_record = self.local_relay_record(now).unwrap_or_else(|e| {
                    warn!("Failed to build local relay record: {e}");
                    None
                });
                let status = NodeStatus {
                    peer_id: self.swarm.local_peer_id().to_string(),
                    identity_address: self.identity.jolt_address().to_string(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    connected_peers: self.swarm.connected_peers().count(),
                    direct_peers: direct,
                    relayed_peers: relayed,
                    nat_type: self.transport_name.to_string(),
                    active_relays: 0,
                    published_count: self.store.published_ids().len(),
                    cached_count: self.store.list_entries().len(),
                    listen_addresses: self.swarm.listeners().map(|a| a.to_string()).collect(),
                    bootstrap_relay: self.bootstrap_relay,
                    bootstrap_state: self.bootstrap_state(connected_bootstrap_peers),
                    configured_bootstrap_relays: self.configured_bootstrap_relays.clone(),
                    configured_bootstrap_relay_count: self.configured_bootstrap_relays.len(),
                    effective_bootstrap_relays: self.effective_bootstrap_relays.clone(),
                    effective_bootstrap_relay_count: self.effective_bootstrap_relays.len(),
                    known_relay_count,
                    connected_bootstrap_peers,
                    last_bootstrap_error: self.last_bootstrap_error.clone(),
                    home_relay: self.home_relay.clone(),
                    relay_record,
                };
                let _ = response_tx.send(status);
            }
            DaemonCommand::GetPeers { response_tx } => {
                let peers = self
                    .swarm
                    .connected_peers()
                    .map(|p| {
                        let conn = self.peer_connections.get(p);
                        PeerInfo {
                            peer_id: p.to_string(),
                            is_relayed: conn.map(|c| c.is_relayed).unwrap_or(false),
                            transport: conn
                                .map(|c| c.transport.clone())
                                .unwrap_or_else(|| self.transport_name.to_string()),
                            remote_addr: conn.map(|c| c.remote_addr.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                let _ = response_tx.send(peers);
            }
            DaemonCommand::GetCacheStats { response_tx } => {
                let stats = self.store.stats();
                let _ = response_tx.send(CacheStatsResponse {
                    total_cached: stats.total_cached,
                    total_published: stats.total_published,
                    cached_items: stats.cached_items,
                    published_items: stats.published_items,
                    pinned_items: stats.pinned_items,
                    pinned_size: stats.pinned_size,
                    max_size: stats.max_size,
                    available: stats.available,
                });
            }
            DaemonCommand::ListCacheEntries { response_tx } => {
                let entries = self
                    .store
                    .list_entries()
                    .into_iter()
                    .map(|e| CacheEntryInfo {
                        content_id: e.content_id.clone(),
                        size: e.size,
                        cached_at: e.cached_at,
                        last_accessed: e.last_accessed,
                        pinned: e.pinned,
                    })
                    .collect();
                let _ = response_tx.send(entries);
            }
            DaemonCommand::ListPublishedContent { response_tx } => {
                let _ = response_tx.send(self.published_content_inventory());
            }
            DaemonCommand::Pin {
                content_id,
                response_tx,
            } => {
                let result = self
                    .store
                    .pin(&content_id)
                    .map_err(|e| NetworkError::Protocol(e.to_string()))
                    .and_then(|_| {
                        let content_id = ContentId::from_str(&content_id)
                            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
                        self.announce_provider(&content_id)
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::CreatePinRequest {
                content_id,
                response_tx,
            } => {
                let result = ContentId::from_str(&content_id)
                    .map_err(|e| NetworkError::InvalidInput(e.to_string()))
                    .and_then(|parsed| {
                        if !self
                            .store
                            .published_ids()
                            .iter()
                            .any(|published| published == &content_id)
                        {
                            return Err(NetworkError::InvalidInput(format!(
                                "content is not locally published: {content_id}"
                            )));
                        }

                        let update_log_content_id =
                            self.publish_update_log_snapshot(&self.identity.identity_id())?;

                        PinRequest::with_update_log(
                            self.identity.public_key_bytes(),
                            parsed,
                            update_log_content_id,
                            |bytes| self.identity.sign(bytes),
                        )
                        .map_err(|e| NetworkError::Protocol(e.to_string()))
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::RecordHomeRelayPin {
                content_id,
                path,
                relay,
                latest_sequence,
                response_tx,
            } => {
                let result = self.record_home_relay_pin(&content_id, path, relay, latest_sequence);
                let _ = response_tx.send(result);
            }
            DaemonCommand::UpdateNetworkSettings {
                configured_bootstrap_relays,
                effective_bootstrap_relays,
                home_relay,
                response_tx,
            } => {
                self.bootstrap_peer_ids =
                    Self::parse_bootstrap_peer_ids(&effective_bootstrap_relays);
                self.configured_bootstrap_relays = configured_bootstrap_relays;
                self.effective_bootstrap_relays = effective_bootstrap_relays;
                self.home_relay = home_relay;
                let _ = response_tx.send(Ok(()));
            }
            DaemonCommand::PinUpdateLog {
                identity,
                response_tx,
            } => {
                if let Some(entries) = self.update_logs.get(&identity) {
                    let result = verify_update_log_for_identity(&identity, entries)
                        .map_err(|e| NetworkError::Protocol(e.to_string()))
                        .and_then(|sequence| {
                            self.announce_update_log_provider(&identity)?;
                            Ok(sequence)
                        });
                    let _ = response_tx.send(result);
                    return;
                }

                let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
                let Some(peer) = peers.first() else {
                    let _ = response_tx.send(Err(NetworkError::NoPeers));
                    return;
                };

                let request = UpdateLogRequest {
                    identity: identity.clone(),
                    since: None,
                };
                let request_id = self
                    .swarm
                    .behaviour_mut()
                    .update_log_sync
                    .send_request(peer, request);
                self.pending_update_log_pins.insert(
                    request_id,
                    PendingUpdateLogPin {
                        identity,
                        response_tx,
                    },
                );
            }
            DaemonCommand::StoreUpdateLog {
                identity,
                entries,
                response_tx,
            } => {
                let result = self
                    .store_verified_update_log(identity.clone(), entries)
                    .and_then(|_| {
                        let entries = self.update_logs.get(&identity).ok_or_else(|| {
                            NetworkError::Protocol(format!(
                                "No verified update log cached for {identity}"
                            ))
                        })?;
                        let sequence = verify_update_log_for_identity(&identity, entries)
                            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
                        self.announce_update_log_provider(&identity)?;
                        Ok(sequence)
                    });
                let _ = response_tx.send(result);
            }
            DaemonCommand::Unpin {
                content_id,
                response_tx,
            } => {
                let result = self
                    .store
                    .unpin(&content_id)
                    .map_err(|e| NetworkError::Protocol(e.to_string()));
                let _ = response_tx.send(result);
            }
            DaemonCommand::Shutdown { response_tx } => {
                let _ = response_tx.send(());
            }
        }
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

    /// Set the timeout for daemon `.jolt` provider discovery.
    pub fn set_resolve_timeout(&mut self, timeout: Duration) {
        self.resolve_timeout = timeout;
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
    use jolt_core::{UpdateAction, UpdateLogEntry};
    use jolt_store::CacheConfig;
    use std::str::FromStr;
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

    fn make_node_with_config(dir: &std::path::Path, config: NetworkConfig) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, config).unwrap()
    }

    fn node_identity_id_from_status(address: &str) -> IdentityId {
        JoltAddress::from_str(address).unwrap().identity().clone()
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
    async fn new_tcp_creates_node_without_error() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config());
        assert!(node.is_ok());
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
        node.enforce_identity_head_hint_global_bound_with_limit(4);

        let stored_count: usize = node.identity_head_hints.values().map(Vec::len).sum();
        assert_eq!(stored_count, 4);
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
    async fn daemon_resolution_retries_next_update_log_provider_after_dial_failure() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let address = JoltAddress::new(owner.identity_id(), "/profile").unwrap();
        let failed_provider = libp2p::PeerId::random();
        let fallback_provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(address.identity());
        node.discovered_providers
            .insert(key, vec![failed_provider, fallback_provider]);

        let (tx, mut rx) = oneshot::channel();
        node.request_daemon_resolve_from_provider(
            address.clone(),
            None,
            &failed_provider,
            tx,
            None,
        );
        let failed_request_id = *node.pending_daemon_resolutions.keys().next().unwrap();

        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
            request_response::Event::OutboundFailure {
                peer: failed_provider,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                request_id: failed_request_id,
                error: request_response::OutboundFailure::DialFailure,
            },
        )));

        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        let pending = node.pending_daemon_resolutions.values().next().unwrap();
        assert_eq!(pending.provider, fallback_provider);
    }

    #[tokio::test]
    async fn daemon_resolution_refreshes_cached_address_when_provider_is_known() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let identity = owner.identity_id();
        let path = "/hello/1234";
        let old_content_id = ContentId::from_bytes(b"old content");
        let new_content_id = ContentId::from_bytes(b"new content");
        let genesis = UpdateLogEntry::genesis(
            owner.public_key_bytes(),
            UpdateAction::SetPath {
                path: path.to_string(),
                content_id: old_content_id.clone(),
            },
            |bytes| owner.sign(bytes),
        )
        .unwrap();
        let newer = genesis
            .append(
                UpdateAction::SetPath {
                    path: path.to_string(),
                    content_id: new_content_id.clone(),
                },
                |bytes| owner.sign(bytes),
            )
            .unwrap();
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        let address = JoltAddress::new(identity.clone(), path).unwrap();

        node.store_verified_update_log(identity.clone(), vec![genesis.clone()])
            .unwrap();
        node.discovered_providers.insert(key, vec![provider]);

        let (tx, mut rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });

        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        let request_id = *node.pending_daemon_resolutions.keys().next().unwrap();

        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
            request_response::Event::Message {
                peer: provider,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                message: request_response::Message::Response {
                    request_id,
                    response: UpdateLogResponse {
                        entries: vec![genesis, newer],
                    },
                },
            },
        )));

        let resolved = rx.await.unwrap().unwrap();
        assert_eq!(resolved.content_id, new_content_id.to_string());
        assert_eq!(resolved.latest_sequence, 1);
        assert_eq!(resolved.source, "network");
    }

    #[tokio::test]
    async fn daemon_resolution_times_out_when_update_log_provider_stalls() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        node.set_resolve_timeout(Duration::from_millis(0));
        let owner = NodeIdentity::generate();
        let address = JoltAddress::new(owner.identity_id(), "/profile").unwrap();
        let provider = libp2p::PeerId::random();

        let (tx, rx) = oneshot::channel();
        node.request_daemon_resolve_from_provider(address, None, &provider, tx, None);

        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        node.check_resolve_timeouts();

        assert_eq!(node.pending_daemon_resolutions.len(), 0);
        assert!(matches!(rx.await.unwrap(), Err(NetworkError::Timeout)));
    }

    #[tokio::test]
    async fn new_tcp_rejects_invalid_persisted_local_update_log() {
        let dir = tempdir().unwrap();
        let owner = NodeIdentity::generate();
        let attacker = NodeIdentity::generate();
        let owner_identity = owner.identity_id();
        let store = make_store(dir.path());
        store
            .save_update_log(
                &owner_identity,
                &signed_profile_log(&attacker, b"wrong owner"),
            )
            .unwrap();

        let result = NetworkNode::new_tcp(owner, store, NetworkConfig::test_config());

        assert!(matches!(
            result,
            Err(NetworkError::Protocol(message))
                if message.contains("invalid persisted update log")
                    && message.contains(&owner_identity.to_string())
        ));
    }

    #[tokio::test]
    async fn update_log_provider_key_is_derived_from_identity() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let identity = owner.identity_id();
        let expected_key = format!("jolt:update-log:{identity}");

        assert_eq!(
            NetworkNode::update_log_provider_key(&identity),
            expected_key
        );

        node.announce_update_log_provider(&identity).unwrap();
        node.find_update_log_providers(&identity);
        assert!(node
            .take_discovered_update_log_provider(&identity)
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "creates an iroh endpoint and may depend on local network/relay availability"]
    async fn new_iroh_creates_node_without_error() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let node = tokio::time::timeout(
            Duration::from_secs(10),
            NetworkNode::new(identity, store, NetworkConfig::test_config()),
        )
        .await;
        assert!(node.is_ok(), "iroh transport creation timed out");
        assert!(node.unwrap().is_ok());
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
    async fn test_daemon_command_status() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx.clone());

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let status = handle.status().await.unwrap();
        assert!(!status.peer_id.is_empty());
        assert_eq!(status.connected_peers, 0);

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_status_reports_bootstrap_config_and_relay_mode() {
        let dir = tempdir().unwrap();
        let mut config = NetworkConfig::test_config();
        config.configured_bootstrap_relays =
            vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string()];
        config.effective_bootstrap_relays = vec![
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string(),
            "/ip4/127.0.0.1/tcp/4002/p2p/12D3Cli".to_string(),
        ];
        config.bootstrap_relay = true;
        config.home_relay = Some(crate::config::HomeRelayConfig {
            peer_id: "12D3Configured".to_string(),
            multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string(),
            capability: crate::config::HomeRelayCapability::Pinning,
            api_url: Some("http://127.0.0.1:9862".to_string()),
        });
        let mut node = make_node_with_config(dir.path(), config);

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx.clone());

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let status = handle.status().await.unwrap();
        assert!(status.bootstrap_relay);
        assert_eq!(status.bootstrap_state, "bootstrapping");
        assert_eq!(status.configured_bootstrap_relay_count, 1);
        assert_eq!(status.effective_bootstrap_relay_count, 2);
        assert_eq!(status.connected_bootstrap_peers, 0);
        assert_eq!(status.last_bootstrap_error, None);
        assert_eq!(
            status.configured_bootstrap_relays,
            vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured"]
        );
        assert_eq!(
            status.effective_bootstrap_relays,
            vec![
                "/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured",
                "/ip4/127.0.0.1/tcp/4002/p2p/12D3Cli",
            ]
        );
        assert_eq!(
            status.home_relay.unwrap().multiaddr,
            "/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured"
        );
        let relay_record = status
            .relay_record
            .expect("relay mode exposes relay record");
        assert_eq!(relay_record.body.peer_id, status.peer_id);
        assert_eq!(
            relay_record.body.relay_id,
            node_identity_id_from_status(&status.identity_address)
        );
        assert!(relay_record
            .body
            .capabilities
            .contains(&jolt_core::RelayRecordCapability::Bootstrap));
        assert_eq!(relay_record.verify_at(unix_now()), Ok(()));

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_status_reports_degraded_bootstrap_after_error() {
        let dir = tempdir().unwrap();
        let mut config = NetworkConfig::test_config();
        config.effective_bootstrap_relays =
            vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string()];
        let mut node = make_node_with_config(dir.path(), config);
        node.last_bootstrap_error = Some("DHT bootstrap failed".to_string());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx.clone());

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let status = handle.status().await.unwrap();
        assert_eq!(status.bootstrap_state, "degraded");
        assert_eq!(status.connected_bootstrap_peers, 0);
        assert_eq!(
            status.last_bootstrap_error,
            Some("DHT bootstrap failed".to_string())
        );

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

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

    #[tokio::test]
    async fn test_daemon_handle_disconnected() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DaemonCommand>(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);
        drop(cmd_rx);

        let status_err = handle.status().await;
        assert!(status_err.is_err());
    }
}
