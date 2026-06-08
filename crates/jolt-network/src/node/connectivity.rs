use libp2p::multiaddr::Protocol;
use libp2p::Multiaddr;
use tracing::{info, warn};

use jolt_core::{ContentId, IdentityId};

use crate::error::NetworkError;

use super::NetworkNode;

impl NetworkNode {
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

    pub(super) fn peer_hint_multiaddr(remote_addr: &str, peer_id: libp2p::PeerId) -> String {
        if remote_addr.contains("/p2p/") {
            return remote_addr.to_string();
        }

        match remote_addr.parse::<Multiaddr>() {
            Ok(addr) => addr.with(Protocol::P2p(peer_id)).to_string(),
            Err(_) => remote_addr.to_string(),
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

    pub(super) fn take_discovered_update_log_provider_except(
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
}

#[cfg(test)]
mod tests {
    use jolt_identity::NodeIdentity;
    use jolt_store::{CacheConfig, ContentStore};
    use tempfile::tempdir;

    use crate::config::NetworkConfig;
    use crate::node::NetworkNode;

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
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
}
