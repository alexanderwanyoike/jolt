use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use libp2p::futures::StreamExt;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm};
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

        let transport = if remote_addr.contains("quic-v1") {
            "quic".to_string()
        } else if remote_addr.contains("/tcp/") {
            "tcp".to_string()
        } else if remote_addr.contains("webrtc") {
            "webrtc".to_string()
        } else {
            "unknown".to_string()
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
    /// Bootstrap peers (PeerId, full multiaddr) to register relay circuits with once connected.
    pending_relay_peers: Vec<(libp2p::PeerId, Multiaddr)>,
    /// Active relay circuit addresses for periodic reservation renewal.
    relay_circuit_addrs: Vec<Multiaddr>,
    /// Providers discovered via DHT: content_id string -> provider PeerIds
    discovered_providers: HashMap<String, Vec<libp2p::PeerId>>,
    /// Relay circuits that have been confirmed ready (reservation accepted)
    relay_circuits_ready: HashSet<libp2p::PeerId>,
    /// Peers known to have public addresses (potential relays)
    known_relay_peers: HashSet<libp2p::PeerId>,
    /// Connection quality tracking: peer -> connection info
    peer_connections: HashMap<libp2p::PeerId, PeerConnectionInfo>,
    /// Detected NAT type from STUN probing
    nat_type: crate::stun::NatType,
    /// QUIC listener port (for STUN external address construction)
    quic_listen_port: u16,
    /// When the node was created (for uptime reporting)
    started_at: Instant,
    /// Manages in-flight fetch operations for the daemon loop
    fetch_manager: FetchManager,
}

impl NetworkNode {
    /// Create a new network node with the given identity, content store, and config.
    pub async fn new(
        identity: NodeIdentity,
        store: ContentStore,
        _config: NetworkConfig,
    ) -> Result<Self, NetworkError> {
        let libp2p_keypair = identity.to_libp2p_keypair();

        let swarm = libp2p::SwarmBuilder::with_existing_identity(libp2p_keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default().nodelay(true),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_quic()
            .with_other_transport(|keypair| {
                let certificate = libp2p_webrtc::tokio::Certificate::generate(&mut rand::thread_rng())
                    .map_err(|e| std::io::Error::other(e))?;
                Ok(libp2p_webrtc::tokio::Transport::new(keypair.clone(), certificate))
            })
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_dns()
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_relay_client(
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_behaviour(|key, relay_client| {
                let peer_id = key.public().to_peer_id();

                // mDNS (LAN discovery)
                let mdns = libp2p::mdns::tokio::Behaviour::new(
                    libp2p::mdns::Config::default(),
                    peer_id,
                )?;

                // Content fetch protocol (unchanged)
                let content_fetch = request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new("/dweb/content/1.0.0"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                );

                // Kademlia DHT
                let mut kad_config = libp2p::kad::Config::new(
                    StreamProtocol::new("/dweb/kad/1.0.0"),
                );
                kad_config.set_query_timeout(Duration::from_secs(60));
                let kad_store = libp2p::kad::store::MemoryStore::new(peer_id);
                let kademlia =
                    libp2p::kad::Behaviour::with_config(peer_id, kad_store, kad_config);

                // Identify (peer info exchange)
                let identify = libp2p::identify::Behaviour::new(
                    libp2p::identify::Config::new(
                        "/dweb/id/1.0.0".to_string(),
                        key.public(),
                    ),
                );

                // AutoNAT (NAT detection)
                let autonat = libp2p::autonat::Behaviour::new(
                    peer_id,
                    libp2p::autonat::Config::default(),
                );

                // Relay server (allows this node to relay for others)
                // Increase limits from defaults to support dcutr hole punching
                // alongside content transfer on the same relay connections
                let relay_config = libp2p::relay::Config {
                    max_reservations: 256,
                    max_reservations_per_peer: 8,
                    max_circuits: 64,
                    max_circuits_per_peer: 8,
                    max_circuit_duration: std::time::Duration::from_secs(5 * 60),
                    max_circuit_bytes: 1 << 20, // 1 MB
                    ..Default::default()
                };
                let relay_server = libp2p::relay::Behaviour::new(peer_id, relay_config);

                // dcutr (hole punching)
                let dcutr = libp2p::dcutr::Behaviour::new(peer_id);

                // UPnP (automatic port mapping)
                let upnp = libp2p::upnp::tokio::Behaviour::default();

                Ok(DwebBehaviour {
                    mdns,
                    content_fetch,
                    kademlia,
                    identify,
                    autonat,
                    relay_client,
                    relay_server,
                    dcutr,
                    upnp,
                })
            })
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(300)))
            .build();

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
            pending_relay_peers: Vec::new(),
            relay_circuit_addrs: Vec::new(),
            discovered_providers: HashMap::new(),
            relay_circuits_ready: HashSet::new(),
            known_relay_peers: HashSet::new(),
            peer_connections: HashMap::new(),
            nat_type: crate::stun::NatType::Unknown,
            quic_listen_port: 0,
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

        // Track the QUIC listener port for STUN discovery
        if addr.contains("quic-v1") && !addr.contains("127.0.0.1") && !addr.contains("::1") {
            if let Some(port) = multiaddr.iter().find_map(|p| match p {
                libp2p::multiaddr::Protocol::Udp(port) => Some(port),
                _ => None,
            }) {
                if port > 0 {
                    self.quic_listen_port = port;
                }
            }
        }

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

        // Announce as DHT provider (best-effort, don't fail publish on DHT error)
        if let Err(e) = self.announce_provider(&content_id) {
            debug!("DHT announcement skipped: {e}");
        }

        Ok(content_id)
    }

    /// Request content from connected peers by ContentId.
    /// Sends to the first connected peer. Use `request_content_from` for a specific peer.
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
            // Parse and generate both TCP and QUIC variants for better connectivity
            let variants = crate::bootstrap::parse_bootstrap_addr_with_quic(&addr.to_string())?;
            let peer_id = variants[0].0;

            for (_, transport) in &variants {
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, transport.clone());
                if let Err(e) = self.swarm.dial(transport.clone()) {
                    debug!("Failed to dial bootstrap peer {peer_id} at {transport}: {e}");
                }
            }
            // Store for relay circuit registration once connected
            self.pending_relay_peers.push((peer_id, addr.clone()));
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
                // Feed addresses into Kademlia
                let mut has_public_addr = false;
                for addr in &info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());

                    // Detect if this peer has a public (non-private) IP
                    let addr_str = addr.to_string();
                    if !addr_str.contains("/ip4/10.")
                        && !addr_str.contains("/ip4/172.")
                        && !addr_str.contains("/ip4/192.168.")
                        && !addr_str.contains("/ip4/127.")
                        && !addr_str.contains("/ip6/::1")
                        && !addr_str.contains("/ip6/fe80")
                        && (addr_str.contains("/ip4/") || addr_str.contains("/ip6/"))
                    {
                        has_public_addr = true;
                    }
                }

                // Track peers with public IPs as potential relays
                if has_public_addr && !self.known_relay_peers.contains(&peer_id) {
                    info!("Discovered public peer {peer_id} (potential relay)");
                    self.known_relay_peers.insert(peer_id);

                    // Request relay reservation if we don't already have one for this peer
                    let already_has_reservation = self.relay_circuits_ready.contains(&peer_id)
                        || self.relay_circuit_addrs.iter().any(|a| a.to_string().contains(&peer_id.to_string()));
                    if !already_has_reservation && self.relay_circuit_addrs.len() < 3 {
                        if let Some(public_addr) = info.listen_addrs.iter().find(|a| {
                            let s = a.to_string();
                            !s.contains("127.") && !s.contains("10.") && !s.contains("192.168.")
                                && !s.contains("172.") && s.contains("/udp/")
                        }) {
                            let circuit_addr = public_addr.clone()
                                .with(libp2p::multiaddr::Protocol::P2p(peer_id))
                                .with(libp2p::multiaddr::Protocol::P2pCircuit);
                            match self.swarm.listen_on(circuit_addr.clone()) {
                                Ok(_) => {
                                    info!("Requested relay reservation via public peer {peer_id}");
                                    self.relay_circuit_addrs.push(circuit_addr);
                                }
                                Err(e) => debug!("Relay reservation via {peer_id} failed: {e}"),
                            }
                        }
                    }
                }

                // Add our observed address as external (needed for relay server)
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

                                                // If fetch manager is waiting for a provider,
                                                // record it and dial (DON'T send request yet --
                                                // wait for connection + relay settle)
                                                if self.fetch_manager.is_awaiting_provider(&key_str) {
                                                    let already_connected = self.swarm.connected_peers()
                                                        .any(|p| *p == provider);
                                                    self.fetch_manager
                                                        .on_provider_discovered(&key_str, provider, already_connected);
                                                }

                                                // Try direct dial (may succeed if addresses are cached)
                                                let _ = self.swarm.dial(provider);

                                                // ALWAYS also try relay circuits -- direct dial may have
                                                // queued with stale cached addresses that will fail later
                                                let local_peer = *self.swarm.local_peer_id();
                                                let circuit_addrs: Vec<Multiaddr> = self.swarm.listeners()
                                                    .filter(|a| a.to_string().contains("p2p-circuit"))
                                                    .cloned()
                                                    .collect();
                                                for relay_addr in circuit_addrs {
                                                    let circuit_addr = relay_addr
                                                        .to_string()
                                                        .replace(&format!("/p2p/{}", local_peer), &format!("/p2p/{}", provider));
                                                    if let Ok(addr) = circuit_addr.parse::<Multiaddr>() {
                                                        info!("Dialing provider {provider} via relay circuit: {addr}");
                                                        if let Err(e) = self.swarm.dial(addr) {
                                                            debug!("Failed to dial provider via relay: {e}");
                                                        }
                                                    }
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

            // --- AutoNAT ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Autonat(
                libp2p::autonat::Event::StatusChanged { old, new },
            )) => {
                info!("NAT status: {old:?} -> {new:?}");
            }

            // --- Relay Client ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::RelayClient(event)) => {
                match &event {
                    libp2p::relay::client::Event::ReservationReqAccepted {
                        relay_peer_id, ..
                    } => {
                        info!("Relay reservation accepted by {relay_peer_id}");
                        self.relay_circuits_ready.insert(*relay_peer_id);
                    }
                    libp2p::relay::client::Event::InboundCircuitEstablished {
                        src_peer_id, ..
                    } => {
                        info!("Inbound relay circuit from {src_peer_id}");
                    }
                    libp2p::relay::client::Event::OutboundCircuitEstablished {
                        relay_peer_id, ..
                    } => {
                        info!("Outbound relay circuit via {relay_peer_id}");
                    }
                    _ => {
                        info!("Relay event: {event:?}");
                    }
                }
            }

            // --- Relay Server ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::RelayServer(event)) => {
                info!("Relay server event: {event:?}");
            }

            // --- dcutr ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Dcutr(event)) => {
                match event.result {
                    Ok(_) => {
                        info!("Direct connection established via hole punch with {}", event.remote_peer_id);
                        // Upgrade connection tracking: relayed -> direct
                        if let Some(conn) = self.peer_connections.get_mut(&event.remote_peer_id) {
                            conn.is_relayed = false;
                            conn.transport = "quic (hole-punched)".to_string();
                        }
                    }
                    Err(ref e) => {
                        warn!("Hole punch failed with {}: {e}", event.remote_peer_id);
                    }
                }
            }

            // --- UPnP ---
            SwarmEvent::Behaviour(DwebBehaviourEvent::Upnp(event)) => {
                match event {
                    libp2p::upnp::Event::NewExternalAddr(addr) => {
                        info!("UPnP mapped external address: {addr}");
                    }
                    libp2p::upnp::Event::ExpiredExternalAddr(addr) => {
                        debug!("UPnP mapping expired: {addr}");
                    }
                    libp2p::upnp::Event::GatewayNotFound => {
                        debug!("UPnP gateway not found");
                    }
                    libp2p::upnp::Event::NonRoutableGateway => {
                        debug!("UPnP gateway is not routable");
                    }
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

                // If this is a bootstrap peer, register a relay circuit reservation
                if let Some(idx) = self.pending_relay_peers.iter().position(|(pid, _)| pid == &peer_id) {
                    let (_, relay_full_addr) = self.pending_relay_peers.remove(idx);
                    // Append /p2p-circuit to the full relay multiaddr
                    let circuit_addr = relay_full_addr
                        .with(libp2p::multiaddr::Protocol::P2pCircuit);
                    match self.swarm.listen_on(circuit_addr.clone()) {
                        Ok(_) => {
                            info!("Requested relay reservation via {peer_id}");
                            // Store for periodic renewal
                            if !self.relay_circuit_addrs.contains(&circuit_addr) {
                                self.relay_circuit_addrs.push(circuit_addr);
                            }
                        }
                        Err(e) => warn!("Failed to request relay reservation via {peer_id}: {e}"),
                    }
                }
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
    ///
    /// Returns when a `Shutdown` command is received or the command channel closes.
    pub async fn run_daemon_loop(&mut self, mut cmd_rx: mpsc::Receiver<DaemonCommand>) {
        let mut timeout_interval = tokio::time::interval(Duration::from_secs(1));
        let mut relay_renewal = tokio::time::interval(Duration::from_secs(60));
        relay_renewal.tick().await; // skip first immediate tick
        let mut stun_interval = tokio::time::interval(Duration::from_secs(300)); // 5 min refresh
        let mut stun_pending = false;

        let quic_port = self.quic_listen_port;

        // Kick off initial STUN discovery (non-blocking via channel)
        let (stun_tx, mut stun_rx) = mpsc::channel::<crate::stun::StunResult>(1);
        {
            let tx = stun_tx.clone();
            tokio::spawn(async move {
                let servers = crate::stun::resolve_stun_servers().await;
                let result = crate::stun::discover_external_addr(quic_port, &servers).await;
                let _ = tx.send(result).await;
            });
            stun_pending = true;
        }

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
                Some(result) = stun_rx.recv() => {
                    self.apply_stun_result(result);
                }
                _ = stun_interval.tick() => {
                    if !stun_pending {
                        let tx = stun_tx.clone();
                        let port = quic_port;
                        tokio::spawn(async move {
                            let servers = crate::stun::resolve_stun_servers().await;
                            let result = crate::stun::discover_external_addr(port, &servers).await;
                            let _ = tx.send(result).await;
                        });
                        stun_pending = true;
                    }
                }
                _ = relay_renewal.tick() => {
                    // Renew only the FIRST relay reservation to prevent expiry
                    // (avoid flooding the relay server with multiple requests)
                    if let Some(addr) = self.relay_circuit_addrs.first().cloned() {
                        match self.swarm.listen_on(addr) {
                            Ok(_) => debug!("Relay reservation renewal requested"),
                            Err(e) => debug!("Relay reservation renewal failed: {e}"),
                        }
                    }
                }
                _ = timeout_interval.tick() => {
                    self.fetch_manager.check_timeouts();

                    // Send content requests for providers that have connected
                    // and the relay settle delay has passed
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

                // Also start DHT provider query immediately (don't wait for peer failures)
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
                    nat_type: format!("{:?}", self.nat_type),
                    active_relays: self.relay_circuits_ready.len(),
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
                            is_relayed: conn.map(|c| c.is_relayed).unwrap_or(true),
                            transport: conn.map(|c| c.transport.clone()).unwrap_or_else(|| "unknown".to_string()),
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

    /// Check if a relay circuit is confirmed ready.
    pub fn is_relay_ready(&self, relay_peer: &libp2p::PeerId) -> bool {
        self.relay_circuits_ready.contains(relay_peer)
    }

    /// Check if any relay circuit is ready.
    pub fn has_ready_relay(&self) -> bool {
        !self.relay_circuits_ready.is_empty()
    }

    /// Get the listeners' addresses.
    pub fn listeners(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    /// Apply STUN discovery result: update NAT type and add external address.
    ///
    /// For endpoint-independent NAT, the NAT maps the same local port to the same
    /// external port regardless of destination. So external_ip:local_quic_port is
    /// correct. This gives dcutr a non-relay address to exchange for hole punching.
    fn apply_stun_result(&mut self, result: crate::stun::StunResult) {
        self.nat_type = result.nat_type.clone();

        if let Some(addr) = result.external_addr {
            info!("STUN: external IP {}, NAT type: {:?}", addr.ip(), self.nat_type);

            // Only add external address for endpoint-independent NAT where
            // local_port maps consistently to the same external port
            if self.nat_type == crate::stun::NatType::EndpointIndependent && self.quic_listen_port > 0 {
                let multiaddr: Multiaddr = format!("/ip4/{}/udp/{}/quic-v1", addr.ip(), self.quic_listen_port)
                    .parse()
                    .unwrap();
                self.swarm.add_external_address(multiaddr.clone());
                info!("STUN: added external address {multiaddr} (endpoint-independent NAT)");
            }
        }
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

    fn make_store(dir: &Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    async fn make_node(dir: &Path) -> NetworkNode {
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
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let mut node = NetworkNode::new(identity, store, NetworkConfig::test_config())
            .await
            .unwrap();

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"hello dweb").unwrap();

        let content_id = node.publish_file(&test_file).unwrap();
        assert!(content_id.verify(b"hello dweb"));
    }

    #[tokio::test]
    async fn publish_file_creates_manifest_with_valid_signature() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let pubkey = identity.public_key_bytes();
        let store = make_store(dir.path());
        let mut node = NetworkNode::new(identity, store, NetworkConfig::test_config())
            .await
            .unwrap();

        let test_file = dir.path().join("test.txt");
        let data = b"content to sign";
        std::fs::write(&test_file, data).unwrap();

        let content_id = node.publish_file(&test_file).unwrap();

        let content_data = node.store_mut().get_content(&content_id.to_string()).unwrap();
        let valid =
            dweb_identity::verify_signature(&pubkey, data, &content_data.signature).unwrap();
        assert!(valid);
        assert_eq!(content_data.publisher_key, pubkey.to_vec());
    }

    // --- Phase 1: Daemon command channel tests ---

    #[tokio::test]
    async fn test_daemon_command_status() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx.clone());

        // Run daemon loop in background
        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let status = handle.status().await.unwrap();
        assert!(!status.peer_id.is_empty());
        assert_eq!(status.connected_peers, 0);
        assert_eq!(status.published_count, 0);

        // Shut down
        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_command_publish() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"hello daemon").unwrap();

        let response = handle.publish(test_file).await.unwrap();
        assert!(!response.content_id.is_empty());
        assert_eq!(response.size, 12); // "hello daemon" = 12 bytes

        // Verify the content ID matches expected hash
        let expected = ContentId::from_bytes(b"hello daemon");
        assert_eq!(response.content_id, expected.to_string());

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_command_cache_stats() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let stats = handle.cache_stats().await.unwrap();
        assert_eq!(stats.cached_items, 0);
        assert_eq!(stats.published_items, 0);
        assert!(stats.max_size > 0);

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

        // Publish
        let test_file = dir.path().join("roundtrip.txt");
        std::fs::write(&test_file, b"roundtrip data").unwrap();
        let pub_resp = handle.publish(test_file).await.unwrap();

        // Fetch the same content back from local store
        let fetch_resp = handle.fetch(pub_resp.content_id.clone()).await.unwrap();
        assert_eq!(fetch_resp.data, b"roundtrip data");
        assert_eq!(fetch_resp.content_id, pub_resp.content_id);
        assert_eq!(fetch_resp.size, 14);

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

        // Shutdown should complete the daemon loop
        handle.shutdown().await.unwrap();

        // The daemon task should finish
        let result = tokio::time::timeout(Duration::from_secs(5), daemon).await;
        assert!(result.is_ok(), "Daemon should have shut down");
        result.unwrap().unwrap();
    }

    #[tokio::test]
    async fn test_fetch_local_content() {
        // Publish content, then fetch via daemon command channel - should return from local store
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        // Publish first
        let test_file = dir.path().join("local_fetch.txt");
        std::fs::write(&test_file, b"fetch me locally").unwrap();
        let pub_resp = handle.publish(test_file).await.unwrap();

        // Fetch should return from local store without any network
        let fetch_resp = handle.fetch(pub_resp.content_id.clone()).await.unwrap();
        assert_eq!(fetch_resp.data, b"fetch me locally");
        assert_eq!(fetch_resp.size, 16);

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_fetch_timeout() {
        // Request non-existent content with no peers - should eventually timeout
        // Note: In the daemon, fetches with no peers go to DHT which will time out
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path()).await;
        // Override fetch manager with short timeout for test
        node.fetch_manager = crate::fetch_manager::FetchManager::with_timeout(Duration::from_millis(100));

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let result = handle.fetch("nonexistent_content_id".to_string()).await;
        assert!(result.is_err());

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn test_daemon_handle_disconnected() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DaemonCommand>(16);
        let handle = crate::daemon_handle::DaemonHandle::new(cmd_tx);

        // Drop the receiver to simulate daemon not running
        drop(cmd_rx);

        // All methods should return errors, not panic
        let status_err = handle.status().await;
        assert!(status_err.is_err());

        let publish_err = handle.publish("/nonexistent".into()).await;
        assert!(publish_err.is_err());

        let fetch_err = handle.fetch("fake_id".to_string()).await;
        assert!(fetch_err.is_err());

        let shutdown_err = handle.shutdown().await;
        assert!(shutdown_err.is_err());
    }
}
