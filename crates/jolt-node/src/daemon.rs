use std::path::{Path, PathBuf};

use crate::config::NodeConfig;

const DEFAULT_API_PORT: u16 = 9862;

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

pub fn find_running_daemons(config: &NodeConfig) -> Vec<DaemonInfo> {
    let mut daemons = Vec::new();
    if let Some(info) = read_daemon_info(config) {
        if is_pid_alive(info.pid) {
            daemons.push(info);
        }
    }

    for info in find_jolt_start_processes() {
        if !daemons.iter().any(|known| known.pid == info.pid) {
            daemons.push(info);
        }
    }

    daemons
}

pub fn find_single_running_daemon(
    config: &NodeConfig,
) -> Result<Option<DaemonInfo>, Vec<DaemonInfo>> {
    let daemons = find_running_daemons(config);
    match daemons.as_slice() {
        [] => Ok(None),
        [daemon] => Ok(Some(daemon.clone())),
        _ => Err(daemons),
    }
}

fn find_jolt_start_processes() -> Vec<DaemonInfo> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let raw = std::fs::read(entry.path().join("cmdline")).ok()?;
            let args = raw
                .split(|byte| *byte == 0)
                .filter(|arg| !arg.is_empty())
                .filter_map(|arg| std::str::from_utf8(arg).ok())
                .collect::<Vec<_>>();
            daemon_info_from_process_args(pid, &args)
        })
        .collect()
}

fn daemon_info_from_process_args(pid: u32, args: &[&str]) -> Option<DaemonInfo> {
    let program = Path::new(args.first()?).file_name()?.to_str()?;
    if program != "jolt" {
        return None;
    }
    if !args.iter().any(|arg| *arg == "start") {
        return None;
    }

    Some(DaemonInfo {
        pid,
        port: api_port_from_args(args).unwrap_or(DEFAULT_API_PORT),
    })
}

fn api_port_from_args(args: &[&str]) -> Option<u16> {
    for (index, arg) in args.iter().enumerate() {
        if *arg == "--api-port" {
            return args.get(index + 1).and_then(|port| port.parse().ok());
        }
        if let Some(port) = arg.strip_prefix("--api-port=") {
            return port.parse().ok();
        }
    }
    None
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

    #[test]
    fn daemon_process_args_detect_jolt_start_with_default_port() {
        let info = daemon_info_from_process_args(42, &["/tmp/jolt", "start"]).unwrap();

        assert_eq!(info.pid, 42);
        assert_eq!(info.port, 9862);
    }

    #[test]
    fn daemon_process_args_detect_jolt_start_with_custom_port() {
        let info = daemon_info_from_process_args(
            42,
            &["target/release/jolt", "start", "--api-port", "9863"],
        )
        .unwrap();

        assert_eq!(info.pid, 42);
        assert_eq!(info.port, 9863);
    }

    #[test]
    fn daemon_process_args_ignore_non_start_commands() {
        assert!(daemon_info_from_process_args(42, &["jolt", "status"]).is_none());
        assert!(daemon_info_from_process_args(42, &["jolt", "stop"]).is_none());
        assert!(daemon_info_from_process_args(42, &["cargo", "test"]).is_none());
    }
}
