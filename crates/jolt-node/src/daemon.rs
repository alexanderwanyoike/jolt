use std::path::PathBuf;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

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

/// A live pid only counts as the daemon when the process behind it really is
/// a `jolt start` process. PIDs recycle: after a crash or SIGKILL the
/// recorded pid can belong to an unrelated process, and treating it as the
/// daemon left `jolt start` refusing forever behind a stale pid file (#207).
fn is_pid_a_jolt_daemon(pid: u32) -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[Pid::from_u32(pid)]),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .without_tasks(),
    );
    let Some(process) = system.process(Pid::from_u32(pid)) else {
        return false;
    };
    let args = process
        .cmd()
        .iter()
        .map(|arg| arg.to_string_lossy())
        .collect::<Vec<_>>();
    let args = args.iter().map(|arg| arg.as_ref()).collect::<Vec<_>>();
    daemon_info_from_process_args(pid, &args).is_some()
}

/// Check if the daemon is actually running: the recorded pid must be alive
/// AND belong to a jolt daemon process.
pub fn is_daemon_running(config: &NodeConfig) -> bool {
    if let Some(info) = read_daemon_info(config) {
        is_pid_a_jolt_daemon(info.pid)
    } else {
        false
    }
}

pub fn find_running_daemons(config: &NodeConfig) -> Vec<DaemonInfo> {
    let mut daemons = Vec::new();
    if let Some(info) = read_daemon_info(config) {
        if is_pid_a_jolt_daemon(info.pid) {
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
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .without_tasks(),
    );

    system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            let args = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>();
            let args = args.iter().map(|arg| arg.as_ref()).collect::<Vec<_>>();
            daemon_info_from_process_args(pid.as_u32(), &args)
        })
        .collect()
}

fn daemon_info_from_process_args(pid: u32, args: &[&str]) -> Option<DaemonInfo> {
    let program = executable_name(args.first()?)?;
    if program != "jolt" && program != "jolt.exe" {
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

fn executable_name(program: &str) -> Option<&str> {
    program
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recycled_pid_of_foreign_process_is_not_the_daemon() {
        // A pid file left behind by a crash can point at a live pid that now
        // belongs to an unrelated process. That must not read as "daemon is
        // running", or `jolt start` refuses forever (#207).
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        let mut foreign = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");

        write_daemon_info(&config, foreign.id(), 9862).unwrap();
        let running = is_daemon_running(&config);

        let _ = foreign.kill();
        let _ = foreign.wait();
        assert!(
            !running,
            "a live non-jolt process behind the pid file must not count as the daemon"
        );
    }

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
    fn daemon_process_args_detect_windows_jolt_start() {
        let info =
            daemon_info_from_process_args(42, &[r"C:\Users\alice\jolt.exe", "start"]).unwrap();

        assert_eq!(info.pid, 42);
        assert_eq!(info.port, 9862);
    }

    #[test]
    fn daemon_process_args_detect_equals_api_port() {
        let info =
            daemon_info_from_process_args(42, &["/usr/local/bin/jolt", "start", "--api-port=9864"])
                .unwrap();

        assert_eq!(info.pid, 42);
        assert_eq!(info.port, 9864);
    }

    #[test]
    fn daemon_process_args_ignore_non_start_commands() {
        assert!(daemon_info_from_process_args(42, &["jolt", "status"]).is_none());
        assert!(daemon_info_from_process_args(42, &["jolt", "stop"]).is_none());
        assert!(daemon_info_from_process_args(42, &["cargo", "test"]).is_none());
    }
}
