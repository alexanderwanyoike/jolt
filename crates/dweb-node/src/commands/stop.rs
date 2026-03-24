use anyhow::Result;
use tracing::info;

use crate::config::NodeConfig;
use crate::daemon;

pub async fn run() -> Result<()> {
    let config = NodeConfig::default_dirs();

    let info = daemon::read_daemon_info(&config)
        .ok_or_else(|| anyhow::anyhow!("No daemon running"))?;

    if !daemon::is_daemon_running(&config) {
        daemon::clear_daemon_info(&config);
        anyhow::bail!("Daemon is not running (stale PID file cleaned up)");
    }

    // Send SIGTERM to the daemon process
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("kill")
            .arg(info.pid.to_string())
            .output()?;

        if output.status.success() {
            info!("Sent stop signal to daemon (PID {})", info.pid);
            println!("Daemon stopped (PID {})", info.pid);
            daemon::clear_daemon_info(&config);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to stop daemon: {stderr}");
        }
    }

    #[cfg(not(unix))]
    {
        anyhow::bail!("Stop command is only supported on Unix systems");
    }

    Ok(())
}
