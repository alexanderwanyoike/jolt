use anyhow::Result;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::{NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run(port: Option<u16>, bootstrap: Vec<String>, no_bootstrap: bool) -> Result<()> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    let identity = NodeIdentity::load_or_generate(&config.identity_dir)?;
    info!("Peer ID: {}", identity.peer_id());

    // Build network config with bootstrap peers
    let mut net_config = NetworkConfig::default();
    if !bootstrap.is_empty() {
        net_config.bootstrap_peers = bootstrap
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
    }

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;

    // Collect published content IDs before passing store to node
    let published_ids: Vec<String> = store.published_ids();

    let mut node = NetworkNode::new(identity, store, net_config).await?;

    let tcp_addr = match port {
        Some(p) => format!("/ip4/0.0.0.0/tcp/{p}"),
        None => "/ip4/0.0.0.0/tcp/0".to_string(),
    };
    let udp_addr = match port {
        Some(p) => format!("/ip4/0.0.0.0/udp/{p}/quic-v1"),
        None => "/ip4/0.0.0.0/udp/0/quic-v1".to_string(),
    };

    node.listen_on(&tcp_addr)?;
    node.listen_on(&udp_addr)?;

    // Attempt NAT-PMP/PCP port mapping in background (alongside UPnP)
    let listen_port = port.unwrap_or(0);
    if listen_port > 0 {
        tokio::spawn(dweb_network::nat::try_all_mappings(listen_port, listen_port));
    }

    // Bootstrap into DHT if not disabled
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

    info!("mDNS discovery active on LAN");
    info!("Published content: {} items", published_ids.len());
    info!("Press Ctrl+C to stop");

    node.run_event_loop().await;

    Ok(())
}
