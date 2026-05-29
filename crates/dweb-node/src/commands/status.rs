use anyhow::Result;

use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

pub async fn run() -> Result<()> {
    let config = NodeConfig::default_dirs();

    match daemon::read_daemon_info(&config) {
        Some(info) => {
            if !daemon::is_daemon_running(&config) {
                daemon::clear_daemon_info(&config);
                println!("Daemon is not running (stale PID file cleaned up)");
                return Ok(());
            }

            let client = DaemonClient::new(info.port);

            match client.status().await {
                Ok(status) => {
                    println!("Daemon running (PID {})", info.pid);
                    println!(
                        "  Peer ID:    {}",
                        status["peer_id"].as_str().unwrap_or("unknown")
                    );
                    println!(
                        "  Jolt:       {}",
                        status["identity_address"].as_str().unwrap_or("unknown")
                    );
                    println!("  API port:   {}", info.port);
                    println!(
                        "  Peers:      {}",
                        status["connected_peers"].as_u64().unwrap_or(0)
                    );
                    println!(
                        "  Published:  {}",
                        status["published_count"].as_u64().unwrap_or(0)
                    );
                    println!(
                        "  Cached:     {}",
                        status["cached_count"].as_u64().unwrap_or(0)
                    );
                    if let Some(home_relay) = status["home_relay"].as_object() {
                        println!(
                            "  Home relay: {}",
                            home_relay
                                .get("multiaddr")
                                .and_then(|value| value.as_str())
                                .unwrap_or("unknown")
                        );
                    } else {
                        println!("  Home relay: not configured");
                    }

                    let uptime = status["uptime_secs"].as_u64().unwrap_or(0);
                    println!("  Uptime:     {}", format_duration(uptime));

                    if let Some(addrs) = status["listen_addresses"].as_array() {
                        if !addrs.is_empty() {
                            println!("  Listening:");
                            for addr in addrs {
                                if let Some(s) = addr.as_str() {
                                    println!("    {s}");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "Daemon PID {} is running but API is not responding: {e}",
                        info.pid
                    );
                }
            }
        }
        None => {
            println!("Daemon is not running. Start with: dweb start");
        }
    }

    Ok(())
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}
