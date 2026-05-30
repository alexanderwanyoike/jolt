use std::path::PathBuf;

use anyhow::Result;
use tracing::info;

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

pub async fn run(target: &str, output: Option<PathBuf>) -> Result<()> {
    let config = NodeConfig::default_dirs();
    let info = daemon::read_daemon_info(&config)
        .ok_or_else(|| anyhow::anyhow!("Daemon not running. Start with: jolt start"))?;

    if !daemon::is_daemon_running(&config) {
        daemon::clear_daemon_info(&config);
        anyhow::bail!("Daemon not running. Start with: jolt start");
    }

    let client = DaemonClient::new(info.port);

    info!("Fetching: {target}");

    let response = client.fetch(target).await?;
    let content_id = response["content_id"].as_str().unwrap_or(target);

    // Extract data from JSON array of bytes
    let data: Vec<u8> = response["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid response: missing data field"))?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0) as u8)
        .collect();

    let output_path = output.unwrap_or_else(|| PathBuf::from(content_id));
    std::fs::write(&output_path, &data)?;
    info!("Saved to: {}", output_path.display());
    println!("Saved {} bytes to {}", data.len(), output_path.display());

    Ok(())
}
