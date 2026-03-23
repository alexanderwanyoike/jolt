use anyhow::Result;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::NetworkNode;
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run() -> Result<()> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    let identity = NodeIdentity::load_or_generate(&config.identity_dir)?;
    info!("Peer ID: {}", identity.peer_id());

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    let mut node = NetworkNode::new(identity, store).await?;

    node.listen_on("/ip4/0.0.0.0/tcp/0")?;
    node.listen_on("/ip4/0.0.0.0/udp/0/quic-v1")?;

    info!("mDNS discovery active on LAN");
    info!("Press Ctrl+C to stop");

    node.run_event_loop().await;

    Ok(())
}
