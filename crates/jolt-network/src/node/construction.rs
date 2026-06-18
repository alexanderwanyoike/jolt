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

        let mut node = Self {
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
        };

        // Rebuild this node's own append-record (device-writer) state from disk
        // so posts/accepted-reply refs published before a restart still
        // enumerate and are served to peers.
        node.load_persisted_local_device_writer_log()?;

        Ok(node)
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

    #[tokio::test]
    async fn local_append_records_survive_node_restart() {
        // Append records (Spoke posts, accepted-reply refs) are written to the
        // local device-writer log. That log must be persisted and rebuilt when
        // the daemon restarts, or the identity's own append records - and the
        // records it serves to peers over device-writer sync - vanish on every
        // restart. Regression: the device-writer log lived only in memory.
        let dir = tempdir().unwrap();
        let key_dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        identity.save(key_dir.path()).unwrap();
        let identity_id = identity.identity_id();
        let file = dir.path().join("record.json");

        // First boot: publish two append records under different prefixes and
        // confirm both enumerate from the live in-memory state.
        {
            let store = make_store(dir.path());
            let mut node =
                NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
            std::fs::write(&file, b"{\"post\":1}").unwrap();
            node.publish_file_appending_path(&file, "/spoke/posts/p1")
                .unwrap();
            std::fs::write(&file, b"{\"accepted\":1}").unwrap();
            node.publish_file_appending_path(&file, "/spoke/accepted/p1/r1")
                .unwrap();
            let records = node
                .enumerate_append_records(&identity_id, "/spoke/")
                .unwrap();
            assert_eq!(records.len(), 2, "both records visible before restart");
        }

        // Restart: a fresh node over the same store with the same identity must
        // rebuild the local device-writer log from disk.
        let reloaded = NodeIdentity::load(key_dir.path()).unwrap();
        let store = make_store(dir.path());
        let node = NetworkNode::new_tcp(reloaded, store, NetworkConfig::test_config()).unwrap();
        let records = node
            .enumerate_append_records(&identity_id, "/spoke/")
            .unwrap();
        let paths: Vec<&str> = records.iter().map(|r| r.path.as_str()).collect();
        assert!(
            paths.contains(&"/spoke/posts/p1") && paths.contains(&"/spoke/accepted/p1/r1"),
            "local append records must survive a daemon restart, got {paths:?}"
        );
    }

    #[tokio::test]
    async fn local_appends_continue_persisted_device_log_after_restart() {
        // After a restart the local device-writer log must be reloaded so that a
        // new append continues the existing per-device chain (monotonic device
        // sequence) rather than starting a fresh genesis that would diverge from
        // the persisted history.
        let dir = tempdir().unwrap();
        let key_dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        identity.save(key_dir.path()).unwrap();
        let identity_id = identity.identity_id();
        let file = dir.path().join("record.json");

        {
            let store = make_store(dir.path());
            let mut node =
                NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
            std::fs::write(&file, b"{\"post\":1}").unwrap();
            let (_, _, seq) = node
                .publish_file_appending_path(&file, "/spoke/posts/p1")
                .unwrap();
            assert_eq!(seq, 0, "first append is device sequence 0");
        }

        let reloaded = NodeIdentity::load(key_dir.path()).unwrap();
        let store = make_store(dir.path());
        let mut node = NetworkNode::new_tcp(reloaded, store, NetworkConfig::test_config()).unwrap();
        std::fs::write(&file, b"{\"post\":2}").unwrap();
        let (_, _, seq) = node
            .publish_file_appending_path(&file, "/spoke/posts/p2")
            .unwrap();
        assert_eq!(
            seq, 1,
            "an append after restart must continue the persisted device chain"
        );
        let records = node
            .enumerate_append_records(&identity_id, "/spoke/")
            .unwrap();
        assert_eq!(records.len(), 2, "both pre- and post-restart records enumerate");
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
