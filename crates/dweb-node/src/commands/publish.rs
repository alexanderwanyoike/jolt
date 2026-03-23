use std::path::Path;

use anyhow::Result;
use tracing::info;

use dweb_identity::NodeIdentity;
use dweb_network::{NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run(file: &Path) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    let identity = NodeIdentity::load_or_generate(&config.identity_dir)?;

    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;
    let mut node = NetworkNode::new(identity, store, NetworkConfig::test_config()).await?;

    let metadata = std::fs::metadata(file)?;
    info!("Publishing: {}", file.display());
    info!("Size: {} bytes", metadata.len());

    let content_id = node.publish_file(file)?;

    println!("{content_id}");
    info!("Content will be served when the node is running (dweb start)");

    Ok(())
}
