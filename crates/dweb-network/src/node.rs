use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use libp2p::futures::StreamExt;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm, Transport};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use dweb_core::{ContentId, ContentManifest};
use dweb_identity::NodeIdentity;
use dweb_store::ContentStore;

use crate::behaviour::{DwebBehaviour, DwebBehaviourEvent};
use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, FetchResult, NodeStatus, PeerInfo,
    PublishResponse,
};
use crate::config::NetworkConfig;
use crate::error::NetworkError;
use crate::fetch_manager::FetchManager;
use crate::protocol::{ContentRequest, ContentResponse};

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
            libp2p::core::ConnectedPoint::Dialer { address, .. } => {
                (address.to_string(), address.to_string().contains("p2p-circuit"))
            }
            libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } => {
                (send_back_addr.to_string(), send_back_addr.to_string().contains("p2p-circuit"))
            }
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
        (String, oneshot::Sender<Result<ContentResponse, NetworkError>>),
    >,
    /// Providers discovered via DHT: content_id string -> provider PeerIds
    discovered_providers: HashMap<String, Vec<libp2p::PeerId>>,
    /// Connection quality tracking: peer -> connection info
    peer_connections: HashMap<libp2p::PeerId, PeerConnectionInfo>,
    /// When the node was created (for uptime reporting)
    started_at: Instant,
    /// Manages in-flight fetch operations for the daemon loop
    fetch_manager: FetchManager,
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
            info!("Binding iroh transport to fixed UDP port {}", config.p2p_port);
            libp2p_iroh::Transport::new_with_port(Some(&libp2p_keypair), config.p2p_port)
                .await
                .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
        } else {
            libp2p_iroh::Transport::new(Some(&libp2p_keypair))
                .await
                .map_err(|e| NetworkError::Swarm(format!("Failed to create iroh transport: {e}")))?
        };

        // Build behaviours (only 4 -- iroh handles all NAT/relay)
        let mdns = libp2p::mdns::tokio::Behaviour::new(
            libp2p::mdns::Config::default(),
            peer_id,
        ).map_err(|e| NetworkError::Swarm(e.to_string()))?;

        let content_fetch = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/dweb/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(
            StreamProtocol::new("/dweb/kad/1.0.0"),
        );
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(
            libp2p::identify::Config::new(
                "/dweb/id/1.0.0".to_string(),
                libp2p_keypair.public(),
            ),
        );

        let behaviour = DwebBehaviour {
            mdns,
            content_fetch,
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

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            discovered_providers: HashMap::new(),
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
        })
    }

    /// Create a new network node with TCP transport (for testing in isolated namespaces).
    ///
    /// Uses noise + yamux over TCP instead of iroh, so it works in environments
    /// without internet access (e.g. patchbay network namespaces).
    pub fn new_tcp(
        identity: NodeIdentity,
        store: ContentStore,
        _config: NetworkConfig,
    ) -> Result<Self, NetworkError> {
        let libp2p_keypair = identity.to_libp2p_keypair();
        let peer_id = libp2p_keypair.public().to_peer_id();

        let transport = libp2p::tcp::tokio::Transport::default()
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(libp2p::noise::Config::new(&libp2p_keypair)
                .map_err(|e| NetworkError::Swarm(e.to_string()))?)
            .multiplex(libp2p::yamux::Config::default())
            .boxed();

        let mdns = libp2p::mdns::tokio::Behaviour::new(
            libp2p::mdns::Config::default(),
            peer_id,
        ).map_err(|e| NetworkError::Swarm(e.to_string()))?;

        let content_fetch = request_response::cbor::Behaviour::new(
            [(
                StreamProtocol::new("/dweb/content/1.0.0"),
                ProtocolSupport::Full,
            )],
            request_response::Config::default(),
        );

        let mut kad_config = libp2p::kad::Config::new(
            StreamProtocol::new("/dweb/kad/1.0.0"),
        );
        kad_config.set_query_timeout(Duration::from_secs(60));
        let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
        let kademlia = libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

        let identify = libp2p::identify::Behaviour::new(
            libp2p::identify::Config::new(
                "/dweb/id/1.0.0".to_string(),
                libp2p_keypair.public(),
            ),
        );

        let behaviour = DwebBehaviour {
            mdns,
            content_fetch,
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

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            discovered_providers: HashMap::new(),
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
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

    /// Get a reference to the content store.
    pub fn store(&self) -> &ContentStore {
        &self.store
    }

    /// Get a mutable reference to the content store.
    pub fn store_mut(&mut self) -> &mut ContentStore {
        &mut self.store
    }

    /// Add bootstrap peers to Kademlia, dial them, and initiate DHT bootstrap.
    pub fn bootstrap_dht(
        &mut self,
        bootstrap_addrs: &[Multiaddr],
    ) -> Result<(), NetworkError> {
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

    /// Query the DHT for providers of the given content.
    pub fn find_providers(&mut self, content_id: &ContentId) -> libp2p::kad::QueryId {
        let key = libp2p::kad::RecordKey::new(&content_id.to_string().into_bytes());
        self.swarm.behaviour_mut().kademlia.get_providers(key)
    }

    /// Handle a single swarm event.
    pub fn handle_swarm_event(&mut self, event: SwarmEvent<DwebBehaviourEvent>) {
        match event {
            // --- mDNS ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Mdns(
                libp2p::mdns::Event::Discovered(peers),
            )) => {
                for (peer_id, addr) in peers {
                    info!("mDNS discovered peer: {peer_id} at {addr}");
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    if let Err(e) = self.swarm.dial(addr) {
                        debug!("Failed to dial discovered peer: {e}");
                    }
                }
            }

            SwarmEvent::Behaviour(DwebBehaviourEvent::Mdns(
                libp2p::mdns::Event::Expired(peers),
            )) => {
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
                    if let Some(content_id_str) = self.fetch_manager.on_content_response(request_id, response.clone()) {
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

            // --- Identify ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Identify(
                libp2p::identify::Event::Received { peer_id, info, .. },
            )) => {
                debug!("Identified peer {peer_id}: {} protocols", info.protocols.len());
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

            SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                let conn_info = PeerConnectionInfo::from_endpoint(&endpoint);
                info!("Connected to peer: {peer_id} (relayed: {}, transport: {})", conn_info.is_relayed, conn_info.transport);

                // Track connection quality (keep best: direct > relayed)
                let dominated = self.peer_connections.get(&peer_id)
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
                response_tx,
            } => {
                let result = self.publish_file(&file_path).map(|content_id| {
                    let size = std::fs::metadata(&file_path)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    PublishResponse {
                        content_id: content_id.to_string(),
                        size,
                    }
                });
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
                info!("Fetch {content_id}: {} connected peers, querying them first", peers.len());
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
            DaemonCommand::GetStatus { response_tx } => {
                let direct = self.peer_connections.values().filter(|c| !c.is_relayed).count();
                let relayed = self.peer_connections.values().filter(|c| c.is_relayed).count();
                let status = NodeStatus {
                    peer_id: self.swarm.local_peer_id().to_string(),
                    uptime_secs: self.started_at.elapsed().as_secs(),
                    connected_peers: self.swarm.connected_peers().count(),
                    direct_peers: direct,
                    relayed_peers: relayed,
                    nat_type: "iroh".to_string(),
                    active_relays: 0,
                    published_count: self.store.published_ids().len(),
                    cached_count: self.store.list_entries().len(),
                    listen_addresses: self.swarm.listeners().map(|a| a.to_string()).collect(),
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
                            transport: conn.map(|c| c.transport.clone()).unwrap_or_else(|| "iroh".to_string()),
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
                    .map_err(|e| NetworkError::Protocol(e.to_string()));
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use dweb_store::CacheConfig;
    use tempfile::tempdir;

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    async fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new(identity, store, NetworkConfig::test_config())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn new_creates_node_without_error() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let node = NetworkNode::new(identity, store, NetworkConfig::test_config()).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn publish_file_returns_valid_content_id() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"hello dweb").unwrap();

        let content_id = node.publish_file(&test_file).unwrap();
        assert!(content_id.verify(b"hello dweb"));
    }

    // --- Daemon command channel tests ---

    #[tokio::test]
    async fn test_daemon_command_status() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

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
    async fn test_daemon_handle_publish_fetch_roundtrip() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let test_file = dir.path().join("roundtrip.txt");
        std::fs::write(&test_file, b"roundtrip data").unwrap();
        let pub_resp = handle.publish(test_file).await.unwrap();

        let fetch_resp = handle.fetch(pub_resp.content_id.clone()).await.unwrap();
        assert_eq!(fetch_resp.data, b"roundtrip data");

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_handle_shutdown() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

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
