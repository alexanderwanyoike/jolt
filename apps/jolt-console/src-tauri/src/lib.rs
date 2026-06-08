use std::{
    fs::OpenOptions,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
};

use serde::Serialize;

const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:9862";
const DEFAULT_API_BIND: &str = "127.0.0.1";

#[tauri::command]
async fn daemon_get(path: String) -> Result<serde_json::Value, String> {
    daemon_request(reqwest::Method::GET, path, None).await
}

#[tauri::command]
async fn daemon_post(
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    daemon_request(reqwest::Method::POST, path, body).await
}

#[tauri::command]
async fn daemon_lifecycle_status(
    lifecycle: tauri::State<'_, Mutex<DaemonLifecycleManager>>,
) -> Result<DaemonLifecycleReport, String> {
    lifecycle_report(&lifecycle).await
}

#[tauri::command]
async fn daemon_lifecycle_start(
    lifecycle: tauri::State<'_, Mutex<DaemonLifecycleManager>>,
) -> Result<DaemonLifecycleReport, String> {
    let report = lifecycle_report(&lifecycle).await?;
    if report.reachability != DaemonReachability::Unavailable {
        return Ok(report);
    }

    let plan = daemon_start_plan(configured_daemon_binary(), &daemon_base_url())?;
    let mut command = Command::new(&plan.program);
    command.args(&plan.args);
    let log_path = daemon_log_path();
    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("failed to open daemon log {}: {error}", log_path.display()))?;
    let log_for_stderr = log
        .try_clone()
        .map_err(|error| format!("failed to prepare daemon stderr log: {error}"))?;
    command
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_for_stderr));

    let child = command.spawn().map_err(|error| {
        let message = format!(
            "failed to start daemon sidecar {}: {error}",
            plan.program.display()
        );
        if let Ok(mut manager) = lifecycle.lock() {
            manager.last_error = Some(message.clone());
        }
        message
    })?;

    let pid = child.id();
    let log_tail = {
        let mut manager = lifecycle
            .lock()
            .map_err(|_| "daemon lifecycle state lock poisoned".to_string())?;
        manager.child = Some(child);
        manager.last_error = None;
        manager.log_path = Some(log_path);
        manager.log_tail()
    };

    Ok(DaemonLifecycleReport {
        daemon_url: daemon_base_url(),
        reachability: DaemonReachability::Unhealthy,
        ownership: DaemonOwnership::Console,
        pid: Some(pid),
        message: "Console started daemon; waiting for health check".to_string(),
        last_error: None,
        log_tail,
    })
}

#[tauri::command]
async fn daemon_lifecycle_stop(
    lifecycle: tauri::State<'_, Mutex<DaemonLifecycleManager>>,
) -> Result<DaemonLifecycleReport, String> {
    stop_owned_daemon(&lifecycle)?;
    lifecycle_report(&lifecycle).await
}

#[tauri::command]
async fn daemon_lifecycle_restart(
    lifecycle: tauri::State<'_, Mutex<DaemonLifecycleManager>>,
) -> Result<DaemonLifecycleReport, String> {
    stop_owned_daemon(&lifecycle)?;
    daemon_lifecycle_start(lifecycle).await
}

async fn daemon_request(
    method: reqwest::Method,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/{}",
        daemon_base_url().trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    let client = reqwest::Client::new();
    let mut request = client
        .request(method, &url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json");
    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request
        .send()
        .await
        .map_err(|error| format!("daemon request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("daemon response read failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("daemon returned {status}: {body}"));
    }

    serde_json::from_str(&body).map_err(|error| format!("daemon returned invalid JSON: {error}"))
}

async fn lifecycle_report(
    lifecycle: &Mutex<DaemonLifecycleManager>,
) -> Result<DaemonLifecycleReport, String> {
    let daemon_url = daemon_base_url();
    let (owned_pid, last_error, log_tail) = {
        let mut manager = lifecycle
            .lock()
            .map_err(|_| "daemon lifecycle state lock poisoned".to_string())?;
        (
            manager.owned_pid(),
            manager.last_error.clone(),
            manager.log_tail(),
        )
    };
    let health = probe_health(&daemon_url).await;

    Ok(match (owned_pid, health) {
        (Some(pid), HealthProbe::Healthy) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Healthy,
            ownership: DaemonOwnership::Console,
            pid: Some(pid),
            message: "Console owns this daemon".to_string(),
            last_error,
            log_tail,
        },
        (Some(pid), HealthProbe::Unhealthy(error)) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Unhealthy,
            ownership: DaemonOwnership::Console,
            pid: Some(pid),
            message: "Console-owned daemon is starting or unhealthy".to_string(),
            last_error: Some(error).or(last_error),
            log_tail,
        },
        (Some(pid), HealthProbe::Unavailable(error)) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Unhealthy,
            ownership: DaemonOwnership::Console,
            pid: Some(pid),
            message: "Console-owned daemon is not responding yet".to_string(),
            last_error: Some(error).or(last_error),
            log_tail,
        },
        (None, HealthProbe::Healthy) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Healthy,
            ownership: DaemonOwnership::External,
            pid: None,
            message: "Connected to an externally started daemon".to_string(),
            last_error,
            log_tail,
        },
        (None, HealthProbe::Unhealthy(error)) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Unhealthy,
            ownership: DaemonOwnership::External,
            pid: None,
            message: "A local daemon endpoint is reachable but unhealthy".to_string(),
            last_error: Some(error).or(last_error),
            log_tail,
        },
        (None, HealthProbe::Unavailable(_)) => DaemonLifecycleReport {
            daemon_url,
            reachability: DaemonReachability::Unavailable,
            ownership: DaemonOwnership::None,
            pid: None,
            message: "No local daemon is responding".to_string(),
            last_error,
            log_tail,
        },
    })
}

async fn probe_health(daemon_url: &str) -> HealthProbe {
    let url = format!("{}/api/v1/health", daemon_url.trim_end_matches('/'));
    match reqwest::Client::new().get(url).send().await {
        Ok(response) if response.status().is_success() => HealthProbe::Healthy,
        Ok(response) => {
            HealthProbe::Unhealthy(format!("health check returned {}", response.status()))
        }
        Err(error) => HealthProbe::Unavailable(format!("health check failed: {error}")),
    }
}

fn stop_owned_daemon(lifecycle: &Mutex<DaemonLifecycleManager>) -> Result<(), String> {
    let mut child = {
        let mut manager = lifecycle
            .lock()
            .map_err(|_| "daemon lifecycle state lock poisoned".to_string())?;
        manager
            .child
            .take()
            .ok_or_else(|| "Console does not own the running daemon".to_string())?
    };

    child
        .kill()
        .map_err(|error| format!("failed to terminate child daemon: {error}"))?;
    child
        .wait()
        .map_err(|error| format!("failed to wait for child daemon exit: {error}"))?;
    Ok(())
}

fn daemon_base_url() -> String {
    std::env::var("JOLT_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string())
}

fn configured_daemon_binary() -> Option<PathBuf> {
    std::env::var_os("JOLT_DAEMON_BINARY").map(PathBuf::from)
}

fn daemon_log_path() -> PathBuf {
    std::env::var_os("JOLT_CONSOLE_DAEMON_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("jolt-console-daemon.log"))
}

#[derive(Debug, PartialEq)]
struct DaemonStartPlan {
    program: PathBuf,
    args: Vec<String>,
}

fn daemon_start_plan(
    configured_binary: Option<PathBuf>,
    daemon_url: &str,
) -> Result<DaemonStartPlan, String> {
    let program = configured_binary.unwrap_or_else(default_sidecar_path);
    let port = api_port_from_daemon_url(daemon_url)?;

    Ok(DaemonStartPlan {
        program,
        args: vec![
            "start".to_string(),
            "--api-port".to_string(),
            port.to_string(),
            "--api-bind".to_string(),
            DEFAULT_API_BIND.to_string(),
        ],
    })
}

fn default_sidecar_path() -> PathBuf {
    let binary = if cfg!(windows) { "jolt.exe" } else { "jolt" };
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join(binary)))
        .unwrap_or_else(|| PathBuf::from(binary))
}

fn api_port_from_daemon_url(daemon_url: &str) -> Result<u16, String> {
    let without_scheme = daemon_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(daemon_url);
    let authority = without_scheme
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("daemon URL has no authority: {daemon_url}"))?;
    let port = authority
        .rsplit_once(':')
        .map(|(_, port)| port)
        .ok_or_else(|| format!("daemon URL has no explicit port: {daemon_url}"))?;

    port.parse::<u16>()
        .map_err(|error| format!("daemon URL has invalid port {port}: {error}"))
}

#[derive(Default)]
struct DaemonLifecycleManager {
    child: Option<Child>,
    last_error: Option<String>,
    log_path: Option<PathBuf>,
}

impl DaemonLifecycleManager {
    fn owned_pid(&mut self) -> Option<u32> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                self.last_error = Some(format!("Console-owned daemon exited with {status}"));
                self.child = None;
                None
            }
            Ok(None) => Some(child.id()),
            Err(error) => {
                self.last_error = Some(format!("failed to inspect child daemon: {error}"));
                self.child = None;
                None
            }
        }
    }

    fn log_tail(&self) -> Vec<String> {
        self.log_path
            .as_ref()
            .map(|path| tail_lines(path, 40))
            .unwrap_or_default()
    }
}

fn tail_lines(path: &PathBuf, line_count: usize) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|content| {
            let lines = content.lines().map(str::to_string).collect::<Vec<_>>();
            let start = lines.len().saturating_sub(line_count);
            lines[start..].to_vec()
        })
        .unwrap_or_default()
}

enum HealthProbe {
    Healthy,
    Unhealthy(String),
    Unavailable(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum DaemonReachability {
    Healthy,
    Unhealthy,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum DaemonOwnership {
    None,
    Console,
    External,
}

#[derive(Debug, Serialize)]
struct DaemonLifecycleReport {
    daemon_url: String,
    reachability: DaemonReachability,
    ownership: DaemonOwnership,
    pid: Option<u32>,
    message: String,
    last_error: Option<String>,
    log_tail: Vec<String>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Mutex::new(DaemonLifecycleManager::default()))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            daemon_get,
            daemon_post,
            daemon_lifecycle_status,
            daemon_lifecycle_start,
            daemon_lifecycle_stop,
            daemon_lifecycle_restart
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Jolt Console");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn daemon_start_plan_uses_configured_binary_and_local_api_port() {
        let plan = daemon_start_plan(
            Some(PathBuf::from("/opt/jolt/bin/jolt")),
            "http://127.0.0.1:9864",
        )
        .unwrap();

        assert_eq!(plan.program, PathBuf::from("/opt/jolt/bin/jolt"));
        assert_eq!(
            plan.args,
            vec!["start", "--api-port", "9864", "--api-bind", "127.0.0.1"]
        );
    }

    #[test]
    fn stop_without_console_owned_child_is_rejected() {
        let lifecycle = Mutex::new(DaemonLifecycleManager::default());

        let error = stop_owned_daemon(&lifecycle).unwrap_err();

        assert_eq!(error, "Console does not own the running daemon");
    }

    #[test]
    fn tail_lines_returns_the_recent_log_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();

        assert_eq!(tail_lines(&path, 2), vec!["two", "three"]);
    }
}
