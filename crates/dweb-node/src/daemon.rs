use std::path::{Path, PathBuf};

use crate::config::NodeConfig;

#[derive(Debug, Clone)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
}

fn pid_path(config: &NodeConfig) -> PathBuf {
    config.data_dir.join("daemon.pid")
}

fn port_path(config: &NodeConfig) -> PathBuf {
    config.data_dir.join("daemon.port")
}

pub fn write_daemon_info(config: &NodeConfig, pid: u32, port: u16) -> std::io::Result<()> {
    std::fs::create_dir_all(&config.data_dir)?;
    std::fs::write(pid_path(config), pid.to_string())?;
    std::fs::write(port_path(config), port.to_string())?;
    Ok(())
}

pub fn read_daemon_info(config: &NodeConfig) -> Option<DaemonInfo> {
    let pid_str = std::fs::read_to_string(pid_path(config)).ok()?;
    let port_str = std::fs::read_to_string(port_path(config)).ok()?;
    let pid = pid_str.trim().parse::<u32>().ok()?;
    let port = port_str.trim().parse::<u16>().ok()?;
    Some(DaemonInfo { pid, port })
}

pub fn clear_daemon_info(config: &NodeConfig) {
    let _ = std::fs::remove_file(pid_path(config));
    let _ = std::fs::remove_file(port_path(config));
}

/// Check if a process with the given PID is still running.
fn is_pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Check if the daemon is actually running (PID alive + port file exists).
pub fn is_daemon_running(config: &NodeConfig) -> bool {
    if let Some(info) = read_daemon_info(config) {
        is_pid_alive(info.pid)
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_daemon_info_write_read_roundtrip() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());

        write_daemon_info(&config, 12345, 9862).unwrap();

        let info = read_daemon_info(&config).unwrap();
        assert_eq!(info.pid, 12345);
        assert_eq!(info.port, 9862);
    }

    #[test]
    fn test_daemon_info_stale_pid() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());

        // Write a PID that doesn't exist (99999999)
        write_daemon_info(&config, 99999999, 9862).unwrap();

        assert!(!is_daemon_running(&config));
    }

    #[test]
    fn test_daemon_info_clear() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());

        write_daemon_info(&config, 12345, 9862).unwrap();
        assert!(read_daemon_info(&config).is_some());

        clear_daemon_info(&config);
        assert!(read_daemon_info(&config).is_none());
    }

    #[test]
    fn test_no_daemon_info() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());

        assert!(read_daemon_info(&config).is_none());
        assert!(!is_daemon_running(&config));
    }
}
