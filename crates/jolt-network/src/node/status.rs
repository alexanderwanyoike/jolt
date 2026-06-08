use tracing::warn;

use crate::command::NodeStatus;

use super::{unix_now, NetworkNode};

impl NetworkNode {
    pub(super) fn build_status(&self) -> NodeStatus {
        let direct = self
            .peer_connections
            .values()
            .filter(|connection| !connection.is_relayed)
            .count();
        let relayed = self
            .peer_connections
            .values()
            .filter(|connection| connection.is_relayed)
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

        NodeStatus {
            daemon_version: env!("CARGO_PKG_VERSION").to_string(),
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
            listen_addresses: self
                .swarm
                .listeners()
                .map(|address| address.to_string())
                .collect(),
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
        }
    }

    pub(super) fn connected_bootstrap_peer_count(&self) -> usize {
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
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use jolt_core::{IdentityId, JoltAddress, RelayRecordCapability};
    use jolt_identity::NodeIdentity;
    use jolt_store::{CacheConfig, ContentStore};
    use tempfile::tempdir;
    use tokio::sync::mpsc;

    use crate::command::DaemonCommand;
    use crate::config::NetworkConfig;
    use crate::daemon_handle::DaemonHandle;
    use crate::node::NetworkNode;

    use super::unix_now;

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

    fn node_identity_id_from_status(address: &str) -> IdentityId {
        JoltAddress::from_str(address).unwrap().identity().clone()
    }

    #[tokio::test]
    async fn daemon_command_status_reports_basic_node_state() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = DaemonHandle::new(cmd_tx.clone());

        let daemon = tokio::spawn(async move {
            node.run_daemon_loop(cmd_rx).await;
        });

        let status = handle.status().await.unwrap();
        assert_eq!(status.daemon_version, env!("CARGO_PKG_VERSION"));
        assert!(!status.peer_id.is_empty());
        assert_eq!(status.connected_peers, 0);

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn daemon_status_reports_bootstrap_config_and_relay_mode() {
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
        let handle = DaemonHandle::new(cmd_tx.clone());

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
            .contains(&RelayRecordCapability::Bootstrap));
        assert_eq!(relay_record.verify_at(unix_now()), Ok(()));

        handle.shutdown().await.unwrap();
        daemon.await.unwrap();
    }

    #[tokio::test]
    async fn daemon_status_reports_degraded_bootstrap_after_error() {
        let dir = tempdir().unwrap();
        let mut config = NetworkConfig::test_config();
        config.effective_bootstrap_relays =
            vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string()];
        let mut node = make_node_with_config(dir.path(), config);
        node.last_bootstrap_error = Some("DHT bootstrap failed".to_string());

        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let handle = DaemonHandle::new(cmd_tx.clone());

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
    async fn daemon_handle_disconnected_status_returns_error() {
        let (cmd_tx, cmd_rx) = mpsc::channel::<DaemonCommand>(16);
        let handle = DaemonHandle::new(cmd_tx);
        drop(cmd_rx);

        let status_err = handle.status().await;
        assert!(status_err.is_err());
    }
}
