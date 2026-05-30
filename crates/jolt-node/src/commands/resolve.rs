use anyhow::Result;
use tracing::info;

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

pub async fn run(address: &str) -> Result<()> {
    let config = NodeConfig::default_dirs();
    let info = daemon::read_daemon_info(&config)
        .ok_or_else(|| anyhow::anyhow!("Daemon not running. Start with: jolt start"))?;

    if !daemon::is_daemon_running(&config) {
        daemon::clear_daemon_info(&config);
        anyhow::bail!("Daemon not running. Start with: jolt start");
    }

    let client = DaemonClient::new(info.port);

    info!("Resolving: {address}");

    let response = client.resolve(address).await?;
    let content_id = response["content_id"].as_str().unwrap_or("<unknown>");
    let identity = response["identity"].as_str().unwrap_or("<unknown>");
    let path = response["path"].as_str().unwrap_or("/");
    let sequence = response["latest_sequence"].as_u64().unwrap_or(0);
    let source = response["source"].as_str().unwrap_or("unknown");

    println!(
        "Address: {}",
        response["address"].as_str().unwrap_or(address)
    );
    println!("Identity: {identity}");
    println!("Path: {path}");
    println!("Sequence: {sequence}");
    println!("Content ID: {content_id}");
    println!("Source: {source}");

    let hints = response["reachability_hints"]
        .as_array()
        .map_or(0, Vec::len);
    println!("Reachability hints: {hints}");

    Ok(())
}
