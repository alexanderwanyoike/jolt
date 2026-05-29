use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::{
    bootstrap::default_bootstrap_peers, DaemonHandle, Multiaddr, NetworkConfig, NetworkNode,
};
use dweb_store::{CacheConfig, ContentStore};

use crate::cli::TransportMode;
use crate::config::{NodeConfig, NodeSettings};
use crate::daemon;

pub async fn run(
    api_port: u16,
    api_bind: &str,
    bootstrap: Vec<String>,
    no_bootstrap: bool,
    p2p_port: u16,
    transport: TransportMode,
) -> Result<()> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    // Check if daemon is already running
    if daemon::is_daemon_running(&config) {
        let info = daemon::read_daemon_info(&config).unwrap();
        anyhow::bail!(
            "Daemon is already running (PID {}, API port {}). Use 'dweb stop' first.",
            info.pid,
            info.port
        );
    }
    daemon::clear_daemon_info(&config);

    let identity = NodeIdentity::load_or_generate(&config.identity_dir)?;
    info!("Peer ID: {}", identity.peer_id());
    let local_identity = identity.identity_id();
    let settings = config.load_settings()?;
    let builtin_bootstrap = default_bootstrap_peers();
    let (net_config, effective_bootstrap) =
        build_network_config(&settings, &bootstrap, &builtin_bootstrap, p2p_port);

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    let published_ids: Vec<String> = store.published_ids();

    let mut node = match transport {
        TransportMode::Iroh => {
            let mut node = NetworkNode::new(identity, store, net_config).await?;
            // Tell iroh transport to accept incoming connections via its Router.
            // The address is ignored by libp2p-iroh but must be a valid multiaddr.
            node.listen_on("/ip4/0.0.0.0/udp/0/quic-v1")?;
            node
        }
        TransportMode::Tcp => {
            let mut node = NetworkNode::new_tcp(identity, store, net_config)?;
            node.listen_on(&format!("/ip4/0.0.0.0/tcp/{p2p_port}"))?;
            node
        }
    };

    // Re-announce all published content as DHT providers
    for content_id_str in &published_ids {
        if let Ok(content_id) = content_id_str.parse::<dweb_core::ContentId>() {
            if let Err(e) = node.announce_provider(&content_id) {
                info!("Provider announcement skipped for {content_id_str}: {e}");
            }
        }
    }

    if node.update_log_entries(&local_identity).is_some() {
        if let Err(e) = node.announce_update_log_provider(&local_identity) {
            info!("Update-log provider announcement skipped for {local_identity}: {e}");
        }
    }

    // Bootstrap into DHT (must happen before daemon loop starts so swarm has peers)
    if !no_bootstrap && !effective_bootstrap.is_empty() {
        let addrs: Vec<Multiaddr> = effective_bootstrap
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
        match node.bootstrap_dht(&addrs) {
            Ok(()) => info!("DHT bootstrap initiated with {} peers", addrs.len()),
            Err(e) => info!("DHT bootstrap skipped: {e}"),
        }
    }

    // Create command channel and daemon handle
    let (cmd_tx, cmd_rx) = mpsc::channel(256);
    let handle = DaemonHandle::new(cmd_tx);

    // Spawn daemon event loop
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });

    // Write PID and port files
    let pid = std::process::id();
    daemon::write_daemon_info(&config, pid, api_port)?;

    info!("mDNS discovery active on LAN");
    info!("Published content: {} items", published_ids.len());
    info!(
        "Configured bootstrap relays: {}",
        settings.bootstrap_relays.len()
    );
    info!("Effective bootstrap relays: {}", effective_bootstrap.len());
    info!("Bootstrap relay mode: {}", settings.bootstrap_relay);
    info!("HTTP API: http://{api_bind}:{api_port}");
    info!("PID: {pid}");
    match transport {
        TransportMode::Iroh => info!("Transport: iroh (automatic DERP relay + hole punching)"),
        TransportMode::Tcp => info!("Transport: tcp (local deterministic demo mode)"),
    }

    let config_for_cleanup = config.clone();
    let shutdown_handle = handle.clone();

    // Start HTTP server (blocks until server stops)
    let server_result =
        dweb_server::server::start_server(shutdown_handle, api_port, api_bind).await;

    daemon::clear_daemon_info(&config_for_cleanup);
    info!("Daemon stopped");

    if let Err(e) = server_result {
        anyhow::bail!("Server error: {e}");
    }

    Ok(())
}

fn build_network_config(
    settings: &NodeSettings,
    cli_bootstrap: &[String],
    builtin_bootstrap: &[String],
    p2p_port: u16,
) -> (NetworkConfig, Vec<String>) {
    let effective_bootstrap = settings.effective_bootstrap_relays(cli_bootstrap, builtin_bootstrap);
    let mut net_config = NetworkConfig::default();
    net_config.p2p_port = p2p_port;
    net_config.configured_bootstrap_relays = settings.bootstrap_relays.clone();
    net_config.effective_bootstrap_relays = effective_bootstrap.clone();
    net_config.bootstrap_relay = settings.bootstrap_relay;
    net_config.home_relay = settings.home_relay.clone();
    net_config.bootstrap_peers = effective_bootstrap
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();

    (net_config, effective_bootstrap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIGURED: &str =
        "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const CLI: &str =
        "/ip4/89.167.68.66/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const BUILTIN: &str =
        "/dns4/bootstrap.jolt.test/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

    #[test]
    fn build_network_config_uses_configured_and_cli_bootstrap_relays() {
        let settings = NodeSettings {
            bootstrap_relays: vec![CONFIGURED.to_string()],
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: true,
            home_relay: Some(dweb_network::HomeRelayConfig {
                peer_id: "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN".to_string(),
                multiaddr: CONFIGURED.to_string(),
                capability: dweb_network::HomeRelayCapability::Pinning,
                api_url: Some("http://127.0.0.1:9862".to_string()),
            }),
        };
        let cli = vec![CLI.to_string()];
        let builtins = vec![BUILTIN.to_string()];

        let (config, effective) = build_network_config(&settings, &cli, &builtins, 4001);

        assert_eq!(effective, vec![CONFIGURED.to_string(), CLI.to_string()]);
        assert_eq!(config.bootstrap_peers.len(), 2);
        assert_eq!(config.configured_bootstrap_relays, vec![CONFIGURED]);
        assert_eq!(
            config.effective_bootstrap_relays,
            vec![CONFIGURED.to_string(), CLI.to_string()]
        );
        assert!(config.bootstrap_relay);
        assert_eq!(config.home_relay, settings.home_relay);
        assert_eq!(config.p2p_port, 4001);
    }

    #[test]
    fn build_network_config_uses_builtin_defaults_when_explicit_relays_absent() {
        let settings = NodeSettings::default();
        let builtins = vec![BUILTIN.to_string()];

        let (config, effective) = build_network_config(&settings, &[], &builtins, 0);

        assert_eq!(effective, vec![BUILTIN.to_string()]);
        assert_eq!(config.bootstrap_peers.len(), 1);
        assert_eq!(config.effective_bootstrap_relays, vec![BUILTIN]);
    }
}
