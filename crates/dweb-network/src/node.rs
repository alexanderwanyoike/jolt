use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use libp2p::futures::StreamExt;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm, Transport};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use dweb_core::{
    resolve_jolt_address, verify_update_log_for_identity, ContentId, ContentManifest, IdentityId,
    JoltAddress, PinRequest, ResolvedJoltTarget, UpdateAction, UpdateLogEntry,
};
use dweb_identity::NodeIdentity;
use dweb_store::ContentStore;

use crate::behaviour::{DwebBehaviour, DwebBehaviourEvent};
use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, FetchResult, NodeStatus,
    PeerConnectResponse, PeerInfo, PublishResponse, ResolveResponse,
};
use crate::config::{HomeRelayConfig, NetworkConfig};
use crate::error::NetworkError;
use crate::fetch_manager::FetchManager;
use crate::protocol::{ContentRequest, ContentResponse, UpdateLogRequest, UpdateLogResponse};

struct PendingResolve {
    address: JoltAddress,
    response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    deadline: Instant,
}

struct PendingUpdateLogPin {
    identity: IdentityId,
    response_tx: oneshot::Sender<Result<u64, NetworkError>>,
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
    swarm: Swarm<DwebBehaviour>,
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
    pending_jolt_resolutions: HashMap<
        OutboundRequestId,
        (
            JoltAddress,
            Option<u64>,
            libp2p::PeerId,
            oneshot::Sender<Result<ResolvedJoltTarget, NetworkError>>,
        ),
    >,
    pending_daemon_resolutions: HashMap<
        OutboundRequestId,
        (
            JoltAddress,
            Option<u64>,
            libp2p::PeerId,
            oneshot::Sender<Result<ResolveResponse, NetworkError>>,
        ),
    >,
    pending_resolves: HashMap<IdentityId, Vec<PendingResolve>>,
    /// Providers discovered via DHT: content_id string -> provider PeerIds
    discovered_providers: HashMap<String, Vec<libp2p::PeerId>>,
    /// Verified update logs by owner identity.
    update_logs: HashMap<IdentityId, Vec<UpdateLogEntry>>,
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
    /// Whether this node is intentionally acting as a bootstrap/discovery relay.
    bootstrap_relay: bool,
    /// User-selected home relay for delegated availability.
    home_relay: Option<HomeRelayConfig>,
}

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
                StreamProtocol::new("/dweb/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let update_log_sync = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/dweb/update-log/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(StreamProtocol::new("/dweb/kad/1.0.0"));
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
            "/dweb/id/1.0.0".to_string(),
            libp2p_keypair.public(),
        ));

        let behaviour = DwebBehaviour {
            mdns,
            content_fetch,
            update_log_sync,
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

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            pending_update_log_requests: HashMap::new(),
            pending_update_log_pins: HashMap::new(),
            pending_jolt_resolutions: HashMap::new(),
            pending_daemon_resolutions: HashMap::new(),
            pending_resolves: HashMap::new(),
            discovered_providers: HashMap::new(),
            update_logs,
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
            resolve_timeout: Duration::from_secs(10),
            iroh_endpoint,
            transport_name: "iroh",
            configured_bootstrap_relays: config.configured_bootstrap_relays,
            effective_bootstrap_relays: config.effective_bootstrap_relays,
            bootstrap_relay: config.bootstrap_relay,
            home_relay: config.home_relay,
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
                StreamProtocol::new("/dweb/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );
        let update_log_sync = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/dweb/update-log/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(StreamProtocol::new("/dweb/kad/1.0.0"));
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(libp2p::identify::Config::new(
            "/dweb/id/1.0.0".to_string(),
            libp2p_keypair.public(),
        ));

        let behaviour = DwebBehaviour {
            mdns,
            content_fetch,
            update_log_sync,
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

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            pending_update_log_requests: HashMap::new(),
            pending_update_log_pins: HashMap::new(),
            pending_jolt_resolutions: HashMap::new(),
            pending_daemon_resolutions: HashMap::new(),
            pending_resolves: HashMap::new(),
            discovered_providers: HashMap::new(),
            update_logs,
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
            resolve_timeout: Duration::from_secs(10),
            iroh_endpoint: None,
            transport_name: "tcp",
            configured_bootstrap_relays: config.configured_bootstrap_relays,
            effective_bootstrap_relays: config.effective_bootstrap_relays,
            bootstrap_relay: config.bootstrap_relay,
            home_relay: config.home_relay,
        })
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

    /// Publish a file to the content store. Returns the ContentId.
    pub fn publish_file(&mut self, file_path: &Path) -> Result<ContentId, NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
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
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_id = self.publish_file(file_path)?;
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

        Ok((content_id, address, latest_sequence))
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
            self.update_logs.insert(identity, entries);
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

    fn request_daemon_resolve_from_provider(
        &mut self,
        address: JoltAddress,
        now: Option<u64>,
        provider: &libp2p::PeerId,
        response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
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
        self.pending_daemon_resolutions
            .insert(request_id, (address, now, *provider, response_tx));
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
            );
        }
    }

    fn check_resolve_timeouts(&mut self) {
        let now = Instant::now();
        let mut empty = Vec::new();

        for (identity, pending) in &mut self.pending_resolves {
            let mut still_waiting = Vec::new();
            for pending in pending.drain(..) {
                if pending.deadline <= now {
                    let _ = pending.response_tx.send(Err(NetworkError::ProviderNotFound(
                        Self::update_log_provider_key(identity),
                    )));
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
    }

    /// Add bootstrap peers to Kademlia, dial them, and initiate DHT bootstrap.
    pub fn bootstrap_dht(&mut self, bootstrap_addrs: &[Multiaddr]) -> Result<(), NetworkError> {
        for addr in bootstrap_addrs {
            let (peer_id, transport) = crate::bootstrap::parse_bootstrap_addr(&addr.to_string())?;
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
                warn!("Failed to dial bootstrap peer {peer_id}: {e}");
            }
            info!("Added bootstrap peer: {peer_id}");
        }

        self.swarm
            .behaviour_mut()
            .kademlia
            .bootstrap()
            .map_err(|e| NetworkError::Dht(format!("Bootstrap failed: {e:?}")))?;

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
        self.swarm.behaviour_mut().kademlia.get_providers(key)
    }

    /// Handle a single swarm event.
    pub fn handle_swarm_event(&mut self, event: SwarmEvent<DwebBehaviourEvent>) {
        match event {
            // --- mDNS ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Mdns(libp2p::mdns::Event::Discovered(
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

            SwarmEvent::Behaviour(DwebBehaviourEvent::Mdns(libp2p::mdns::Event::Expired(
                peers,
            ))) => {
                for (peer_id, _addr) in peers {
                    debug!("mDNS peer expired: {peer_id}");
                }
            }

            // --- Content Fetch ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::ContentFetch(
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

            SwarmEvent::Behaviour(DwebBehaviourEvent::ContentFetch(
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

            SwarmEvent::Behaviour(DwebBehaviourEvent::ContentFetch(
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
            SwarmEvent::Behaviour(DwebBehaviourEvent::UpdateLogSync(
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

            SwarmEvent::Behaviour(DwebBehaviourEvent::UpdateLogSync(
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
                        Err(NetworkError::Protocol(format!(
                            "No update log entries returned for {}",
                            address.identity()
                        )))
                    } else {
                        self.store_verified_update_log(address.identity().clone(), response.entries)
                            .and_then(|_| self.resolve_cached_jolt_address(&address, now))
                    };
                    let _ = tx.send(result);
                } else if let Some((address, now, _provider, tx)) =
                    self.pending_daemon_resolutions.remove(&request_id)
                {
                    let result = if response.entries.is_empty() {
                        Err(NetworkError::Protocol(format!(
                            "No update log entries returned for {}",
                            address.identity()
                        )))
                    } else {
                        self.store_verified_update_log(address.identity().clone(), response.entries)
                            .and_then(|_| {
                                self.resolve_response_from_cache(&address, now, "network")
                            })
                    };
                    let _ = tx.send(result);
                }
            }

            SwarmEvent::Behaviour(DwebBehaviourEvent::UpdateLogSync(
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
                if let Some((address, now, provider, tx)) =
                    self.pending_daemon_resolutions.remove(&request_id)
                {
                    if let Some(next_provider) = self.take_discovered_update_log_provider_except(
                        address.identity(),
                        Some(&provider),
                    ) {
                        self.request_daemon_resolve_from_provider(address, now, &next_provider, tx);
                    } else {
                        let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                    }
                }
            }

            // --- Identify ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Identify(
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
            SwarmEvent::Behaviour(DwebBehaviourEvent::Kademlia(event)) => {
                match event {
                    libp2p::kad::Event::OutboundQueryProgressed { result, .. } => {
                        match result {
                            libp2p::kad::QueryResult::Bootstrap(Ok(ok)) => {
                                info!("DHT bootstrap step: {} remaining", ok.num_remaining);
                            }
                            libp2p::kad::QueryResult::Bootstrap(Err(e)) => {
                                warn!("DHT bootstrap error: {e:?}");
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

                // Track connection quality (keep best: direct > relayed)
                let dominated = self
                    .peer_connections
                    .get(&peer_id)
                    .map(|existing| existing.is_relayed && !conn_info.is_relayed)
                    .unwrap_or(true);
                if dominated {
                    self.peer_connections.insert(peer_id, conn_info);
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

                match self.resolve_response_from_cache(&address, None, "cache") {
                    Ok(response) => {
                        let _ = response_tx.send(Ok(response));
                    }
                    Err(_) => {
                        let identity = address.identity().clone();
                        self.find_update_log_providers(&identity);

                        if let Some(provider) = self.take_discovered_update_log_provider(&identity)
                        {
                            self.request_daemon_resolve_from_provider(
                                address,
                                None,
                                &provider,
                                response_tx,
                            );
                        } else {
                            self.pending_resolves.entry(identity).or_default().push(
                                PendingResolve {
                                    address,
                                    response_tx,
                                    deadline: Instant::now() + self.resolve_timeout,
                                },
                            );
                        }
                    }
                }
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
                    configured_bootstrap_relays: self.configured_bootstrap_relays.clone(),
                    effective_bootstrap_relays: self.effective_bootstrap_relays.clone(),
                    home_relay: self.home_relay.clone(),
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
        loop {
            let event = self.swarm.select_next_some().await;
            self.handle_swarm_event(event);
        }
    }

    /// Poll for a single next swarm event.
    pub async fn next_event(&mut self) -> SwarmEvent<DwebBehaviourEvent> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use dweb_core::{UpdateAction, UpdateLogEntry};
    use dweb_store::CacheConfig;
    use tempfile::tempdir;

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
    }

    fn make_node_with_config(dir: &std::path::Path, config: NetworkConfig) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, config).unwrap()
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
        node.request_daemon_resolve_from_provider(address.clone(), None, &failed_provider, tx);
        let failed_request_id = *node.pending_daemon_resolutions.keys().next().unwrap();

        node.handle_swarm_event(SwarmEvent::Behaviour(DwebBehaviourEvent::UpdateLogSync(
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
        let (_address, _now, provider, _tx) =
            node.pending_daemon_resolutions.values().next().unwrap();
        assert_eq!(*provider, fallback_provider);
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
        std::fs::write(&test_file, b"hello dweb").unwrap();

        let content_id = node.publish_file(&test_file).unwrap();
        assert!(content_id.verify(b"hello dweb"));
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
