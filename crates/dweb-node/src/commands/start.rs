use anyhow::Result;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::NetworkNode;
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run(port: Option<u16>) -> Result<()> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    let identity = NodeIdentity::load_or_generate(&config.identity_dir)?;
    info!("Peer ID: {}", identity.peer_id());

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    let mut node = NetworkNode::new(identity, store).await?;

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

    info!("mDNS discovery active on LAN");
    info!("Press Ctrl+C to stop");

    node.run_event_loop().await;

    Ok(())
}
