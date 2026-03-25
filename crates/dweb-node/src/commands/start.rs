use anyhow::Result;
use tokio::sync::mpsc;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::{DaemonHandle, NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;
use crate::daemon;

pub async fn run(
    api_port: u16,
    api_bind: &str,
    bootstrap: Vec<String>,
    no_bootstrap: bool,
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

    let mut net_config = NetworkConfig::default();
    if !bootstrap.is_empty() {
        net_config.bootstrap_peers = bootstrap
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
    }

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    let published_ids: Vec<String> = store.published_ids();

    let mut node = NetworkNode::new(identity, store, net_config).await?;

    // Tell iroh transport to accept incoming connections
    // The address is informational -- iroh binds its own UDP socket and DERP relay
    node.listen_on("/p2p/0")?;

    // Bootstrap into DHT
    if !no_bootstrap && !bootstrap.is_empty() {
        let addrs: Vec<_> = bootstrap.iter().filter_map(|s| s.parse().ok()).collect();
        match node.bootstrap_dht(&addrs) {
            Ok(()) => info!("DHT bootstrap initiated with {} peers", addrs.len()),
            Err(e) => info!("DHT bootstrap skipped: {e}"),
        }
    }

    // Re-announce all published content as DHT providers
    for content_id_str in &published_ids {
        if let Ok(content_id) = content_id_str.parse::<dweb_core::ContentId>() {
            if let Err(e) = node.announce_provider(&content_id) {
                info!("Provider announcement skipped for {content_id_str}: {e}");
            }
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
    info!("HTTP API: http://{api_bind}:{api_port}");
    info!("PID: {pid}");
    info!("NAT traversal: iroh (automatic DERP relay + hole punching)");

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
