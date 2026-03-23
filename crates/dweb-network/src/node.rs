use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use libp2p::futures::StreamExt;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use dweb_core::{ContentId, ContentManifest};
use dweb_identity::NodeIdentity;
use dweb_store::ContentStore;

use crate::behaviour::{DwebBehaviour, DwebBehaviourEvent};
use crate::error::NetworkError;
use crate::protocol::{ContentRequest, ContentResponse};

pub struct NetworkNode {
    swarm: Swarm<DwebBehaviour>,
    identity: NodeIdentity,
    store: ContentStore,
    pending_fetches: HashMap<
        OutboundRequestId,
        (String, oneshot::Sender<Result<ContentResponse, NetworkError>>),
    >,
}

impl NetworkNode {
    /// Create a new network node with the given identity and content store.
    pub async fn new(
        identity: NodeIdentity,
        store: ContentStore,
    ) -> Result<Self, NetworkError> {
        let libp2p_keypair = identity.to_libp2p_keypair();

        let swarm = libp2p::SwarmBuilder::with_existing_identity(libp2p_keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                libp2p::noise::Config::new,
                libp2p::yamux::Config::default,
            )
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_quic()
            .with_behaviour(|key| {
                let mdns = libp2p::mdns::tokio::Behaviour::new(
                    libp2p::mdns::Config::default(),
                    key.public().to_peer_id(),
                )?;
                let content_fetch = request_response::cbor::Behaviour::new(
                    [(
                        StreamProtocol::new("/dweb/content/1.0.0"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                );
                Ok(DwebBehaviour {
                    mdns,
                    content_fetch,
                })
            })
            .map_err(|e| NetworkError::Swarm(e.to_string()))?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(Self {
            swarm,
            identity,
            store,
            pending_fetches: HashMap::new(),
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

        let (tx, rx) = oneshot::channel();
        let id_str = content_id.to_string();
        let request = ContentRequest {
            content_id: id_str.clone(),
        };

        let peer = peers[0];
        let request_id = self
            .swarm
            .behaviour_mut()
            .content_fetch
            .send_request(&peer, request);
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

    /// Handle a single swarm event.
    pub fn handle_swarm_event(&mut self, event: SwarmEvent<DwebBehaviourEvent>) {
        match event {
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
                if let Some((content_id_str, tx)) = self.pending_fetches.remove(&request_id) {
                    // Auto-cache the received content
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
                if let Some((_content_id, tx)) = self.pending_fetches.remove(&request_id) {
                    let _ = tx.send(Err(NetworkError::Protocol(error.to_string())));
                }
            }

            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address}");
            }

            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                info!("Connected to peer: {peer_id}");
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                debug!("Disconnected from peer: {peer_id}");
            }

            _ => {}
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use dweb_store::CacheConfig;
    use tempfile::tempdir;

    fn make_store(dir: &Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    #[tokio::test]
    async fn new_creates_node_without_error() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let node = NetworkNode::new(identity, store).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn publish_file_returns_valid_content_id() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = make_store(dir.path());
        let mut node = NetworkNode::new(identity, store).await.unwrap();

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
        let mut node = NetworkNode::new(identity, store).await.unwrap();

        let test_file = dir.path().join("test.txt");
        let data = b"content to sign";
        std::fs::write(&test_file, data).unwrap();

        let content_id = node.publish_file(&test_file).unwrap();

        // Verify content is in the store and signature is valid
        let content_data = node.store_mut().get_content(&content_id.to_string()).unwrap();
        let valid =
            dweb_identity::verify_signature(&pubkey, data, &content_data.signature).unwrap();
        assert!(valid);
        assert_eq!(content_data.publisher_key, pubkey.to_vec());
    }
}
