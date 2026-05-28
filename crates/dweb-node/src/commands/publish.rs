use std::path::Path;

use anyhow::Result;
use tracing::info;

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

pub async fn run(file: &Path, path: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("File not found: {}", file.display());
    }

    let config = NodeConfig::default_dirs();
    let info = daemon::read_daemon_info(&config)
        .ok_or_else(|| anyhow::anyhow!("Daemon not running. Start with: dweb start"))?;

    if !daemon::is_daemon_running(&config) {
        daemon::clear_daemon_info(&config);
        anyhow::bail!("Daemon not running. Start with: dweb start");
    }

    let client = DaemonClient::new(info.port);

    let metadata = std::fs::metadata(file)?;
    info!("Publishing: {}", file.display());
    info!("Size: {} bytes", metadata.len());

    let response = client.publish(file, path).await?;

    let content_id = response["content_id"].as_str().unwrap_or("unknown");
    println!("{content_id}");
    if let Some(address) = response["address"].as_str() {
        println!("{address}");
    }
    info!("Content is now being served by the daemon");

    Ok(())
}
