use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use libp2p::futures::StreamExt;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, StreamProtocol, Swarm};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use dweb_core::{ContentId, ContentManifest};
use dweb_identity::NodeIdentity;

use crate::behaviour::{DwebBehaviour, DwebBehaviourEvent};
use crate::error::NetworkError;
use crate::protocol::{ContentRequest, ContentResponse};

pub struct NetworkNode {
    swarm: Swarm<DwebBehaviour>,
    identity: NodeIdentity,
    content_store: PathBuf,
    published_content: HashMap<String, PathBuf>,
    pending_fetches:
        HashMap<OutboundRequestId, oneshot::Sender<Result<ContentResponse, NetworkError>>>,
}

impl NetworkNode {
    /// Create a new network node.
    pub async fn new(
        identity: NodeIdentity,
        content_store: PathBuf,
    ) -> Result<Self, NetworkError> {
        std::fs::create_dir_all(&content_store).map_err(NetworkError::Io)?;

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

        let mut node = Self {
            swarm,
            identity,
            content_store,
            published_content: HashMap::new(),
            pending_fetches: HashMap::new(),
        };

        // Load any existing published content
        node.load_published_content()?;

        Ok(node)
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
        let id_str = content_id.to_string();

        // Create content directory
        let content_dir = self.content_store.join(&id_str);
        std::fs::create_dir_all(&content_dir).map_err(NetworkError::Io)?;

        // Write content
        let content_path = content_dir.join("content");
        std::fs::write(&content_path, &data).map_err(NetworkError::Io)?;

        // Sign the content hash
        let signature = self.identity.sign(data.as_slice());

        // Write manifest
        let manifest = ContentManifest {
            content_id: content_id.clone(),
            size: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
            publisher_key: self.identity.public_key_bytes().to_vec(),
            signature,
        };
        let manifest_json =
            serde_json::to_string_pretty(&manifest).map_err(|e| NetworkError::Protocol(e.to_string()))?;
        std::fs::write(content_dir.join("manifest.json"), manifest_json)
            .map_err(NetworkError::Io)?;

        self.published_content
            .insert(id_str, content_path);

        Ok(content_id)
    }

    /// Request content from connected peers by ContentId.
    /// Returns a oneshot receiver that will resolve when content arrives.
    pub fn request_content(
        &mut self,
        content_id: &ContentId,
    ) -> Result<oneshot::Receiver<Result<ContentResponse, NetworkError>>, NetworkError> {
        let peers: Vec<_> = self.swarm.connected_peers().cloned().collect();
        if peers.is_empty() {
            return Err(NetworkError::NoPeers);
        }

        let (tx, rx) = oneshot::channel();
        let request = ContentRequest {
            content_id: content_id.to_string(),
        };

        // Send to the first connected peer
        let peer = peers[0];
        let request_id = self
            .swarm
            .behaviour_mut()
            .content_fetch
            .send_request(&peer, request);
        self.pending_fetches.insert(request_id, tx);

        Ok(rx)
    }

    /// Load existing published content from the content store directory.
    fn load_published_content(&mut self) -> Result<(), NetworkError> {
        if !self.content_store.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.content_store).map_err(NetworkError::Io)?;
        for entry in entries {
            let entry = entry.map_err(NetworkError::Io)?;
            let content_path = entry.path().join("content");
            if content_path.exists() {
                let id_str = entry
                    .file_name()
                    .to_string_lossy()
                    .to_string();
                self.published_content.insert(id_str, content_path);
            }
        }

        info!(
            "Loaded {} published content items",
            self.published_content.len()
        );
        Ok(())
    }

    /// Handle a single swarm event. Returns true if a fetch was completed.
    pub fn handle_swarm_event(
        &mut self,
        event: SwarmEvent<DwebBehaviourEvent>,
    ) {
        match event {
            SwarmEvent::Behaviour(DwebBehaviourEvent::Mdns(
                libp2p::mdns::Event::Discovered(peers),
            )) => {
                for (peer_id, addr) in peers {
                    info!("mDNS discovered peer: {peer_id} at {addr}");
                    self.swarm.add_peer_address(peer_id, addr.clone());
                    // Dial the peer to establish a connection
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

                let response = if let Some(content_path) =
                    self.published_content.get(&request.content_id)
                {
                    match std::fs::read(content_path) {
                        Ok(data) => {
                            let signature = self.identity.sign(&data);
                            ContentResponse {
                                data,
                                signature,
                                publisher_key: self.identity.public_key_bytes().to_vec(),
                            }
                        }
                        Err(e) => {
                            warn!("Failed to read content: {e}");
                            ContentResponse {
                                data: vec![],
                                signature: vec![],
                                publisher_key: vec![],
                            }
                        }
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
                if let Some(tx) = self.pending_fetches.remove(&request_id) {
                    let _ = tx.send(Ok(response));
                }
            }

            SwarmEvent::Behaviour(DwebBehaviourEvent::ContentFetch(
                request_response::Event::OutboundFailure {
                    request_id, error, ..
                },
            )) => {
                warn!("Outbound request failed: {error}");
                if let Some(tx) = self.pending_fetches.remove(&request_id) {
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
    use tempfile::tempdir;

    #[tokio::test]
    async fn new_creates_node_without_error() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let node = NetworkNode::new(identity, dir.path().join("store")).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn publish_file_returns_valid_content_id() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let mut node = NetworkNode::new(identity, dir.path().join("store"))
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
        let mut node = NetworkNode::new(identity, dir.path().join("store"))
            .await
            .unwrap();

        let test_file = dir.path().join("test.txt");
        let data = b"content to sign";
        std::fs::write(&test_file, data).unwrap();

        let content_id = node.publish_file(&test_file).unwrap();
        let id_str = content_id.to_string();

        // Read the manifest
        let manifest_path = dir.path().join("store").join(&id_str).join("manifest.json");
        let manifest_json = std::fs::read_to_string(manifest_path).unwrap();
        let manifest: ContentManifest = serde_json::from_str(&manifest_json).unwrap();

        // Verify signature
        let valid =
            dweb_identity::verify_signature(&pubkey, data, &manifest.signature).unwrap();
        assert!(valid);
        assert_eq!(manifest.publisher_key, pubkey.to_vec());
    }
}
