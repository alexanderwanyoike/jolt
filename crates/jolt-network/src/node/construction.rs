use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use jolt_identity::NodeIdentity;
use jolt_store::ContentStore;

use crate::config::NetworkConfig;
use crate::error::NetworkError;
use crate::fetch_manager::FetchManager;
use crate::node::identity_heads::IdentityHeadHintBook;
use crate::node::ingress::IngressQueue;
use crate::node::transport::{self, BuiltTransport};

use super::NetworkNode;

impl NetworkNode {
    /// Create a new network node with iroh transport.
    pub async fn new(
        identity: NodeIdentity,
        store: ContentStore,
        config: NetworkConfig,
    ) -> Result<Self, NetworkError> {
        let built = transport::build_iroh_swarm(&identity, &config).await?;
        Self::from_built_transport(identity, store, config, built)
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
        let built = transport::build_tcp_swarm(&identity, &config)?;
        Self::from_built_transport(identity, store, config, built)
    }

    fn from_built_transport(
        identity: NodeIdentity,
        store: ContentStore,
        config: NetworkConfig,
        built: BuiltTransport,
    ) -> Result<Self, NetworkError> {
        let update_logs = Self::load_persisted_local_update_log(&store, &identity)?;
        let bootstrap_peer_ids = Self::parse_bootstrap_peer_ids(&config.effective_bootstrap_relays);
        let local_encryption_key = Self::load_persisted_local_encryption_key(&store, &identity)?;

        Ok(Self {
            swarm: built.swarm,
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
            pending_device_writer_syncs: HashMap::new(),
            pending_device_writer_waiters: HashMap::new(),
            discovered_providers: HashMap::new(),
            update_logs,
            device_writer_states: HashMap::new(),
            local_device_writer_logs: HashMap::new(),
            local_device_authority_records: HashMap::new(),
            identity_head_hints: IdentityHeadHintBook::default(),
            peer_connections: HashMap::new(),
            started_at: Instant::now(),
            fetch_manager: FetchManager::new(),
            resolve_timeout: Duration::from_secs(10),
            iroh_endpoint: built.iroh_endpoint,
            transport_name: built.transport_name,
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
            pending_ingress: IngressQueue::default(),
        })
    }

    pub(super) fn parse_bootstrap_peer_ids(relays: &[String]) -> HashSet<libp2p::PeerId> {
        relays
            .iter()
            .filter_map(|addr| crate::bootstrap::parse_bootstrap_addr(addr).ok())
            .map(|(peer_id, _)| peer_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use jolt_core::{ContentId, UpdateAction, UpdateLogEntry};
    use jolt_identity::NodeIdentity;
    use jolt_store::{CacheConfig, ContentStore};
    use tempfile::tempdir;

    use crate::config::NetworkConfig;
    use crate::error::NetworkError;
    use crate::node::NetworkNode;

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
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
}
