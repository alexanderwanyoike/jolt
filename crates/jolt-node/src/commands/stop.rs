use anyhow::Result;
use tracing::info;

use crate::config::NodeConfig;
use crate::daemon;

pub async fn run() -> Result<()> {
    let config = NodeConfig::default_dirs();

    let info = match daemon::find_single_running_daemon(&config) {
        Ok(Some(info)) => info,
        Ok(None) => {
            if daemon::read_daemon_info(&config).is_some() {
                daemon::clear_daemon_info(&config);
                anyhow::bail!("Jolt is not running (stale PID file cleaned up)");
            }
            anyhow::bail!("No Jolt daemon running");
        }
        Err(daemons) => {
            let summary = daemons
                .iter()
                .map(|daemon| format!("PID {} on API port {}", daemon.pid, daemon.port))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "Multiple Jolt daemons are running ({summary}). Run stop with the same XDG_DATA_HOME as the target daemon."
            );
        }
    };

    // Send SIGTERM to the daemon process
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("kill").arg(info.pid.to_string()).output()?;

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
