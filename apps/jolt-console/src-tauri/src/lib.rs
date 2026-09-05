#[cfg(target_os = "linux")]
mod desktop_integration;

use std::{
    fs::OpenOptions,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
};

use serde::Serialize;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;

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
async fn daemon_delete(path: String) -> Result<serde_json::Value, String> {
    daemon_request(reqwest::Method::DELETE, path, None).await
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
    stop_any_daemon(&lifecycle)?;
    lifecycle_report(&lifecycle).await
}

#[tauri::command]
async fn daemon_lifecycle_restart(
    lifecycle: tauri::State<'_, Mutex<DaemonLifecycleManager>>,
) -> Result<DaemonLifecycleReport, String> {
    stop_any_daemon(&lifecycle)?;
    daemon_lifecycle_start(lifecycle).await
}

/// Stop the daemon whether or not this console instance spawned it. Since the
/// daemon survives console restarts (#207), the console is often attached to
/// a daemon it does not own; `jolt stop` handles that via the pid file.
fn stop_any_daemon(lifecycle: &Mutex<DaemonLifecycleManager>) -> Result<(), String> {
    if stop_owned_daemon(lifecycle).is_ok() {
        return Ok(());
    }
    let program = configured_daemon_binary().unwrap_or_else(default_sidecar_path);
    let output = Command::new(&program)
        .arg("stop")
        .output()
        .map_err(|error| format!("failed to run {} stop: {error}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "jolt stop failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

#[tauri::command]
async fn identity_export_save_file(
    window: tauri::Window,
    identity: String,
    bundle: serde_json::Value,
) -> Result<Option<String>, String> {
    let file_name = format!("{}.jolt-identity", safe_identity_export_filename(&identity));
    let Some(file_path) = window
        .dialog()
        .file()
        .add_filter("Jolt identity", &["jolt-identity", "json"])
        .set_file_name(file_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|error| format!("invalid export file path: {error}"))?;
    let content = serde_json::to_string_pretty(&bundle)
        .map_err(|error| format!("failed to encode identity bundle: {error}"))?;

    jolt_identity::write_identity_export_file(&path, content.as_bytes()).map_err(|error| {
        format!(
            "failed to write identity bundle {}: {error}",
            path.display()
        )
    })?;

    Ok(Some(path.display().to_string()))
}

#[tauri::command]
async fn identity_export_open_file(
    window: tauri::Window,
) -> Result<Option<serde_json::Value>, String> {
    let Some(file_path) = window
        .dialog()
        .file()
        .add_filter("Jolt identity", &["jolt-identity", "json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = file_path
        .into_path()
        .map_err(|error| format!("invalid import file path: {error}"))?;
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read identity bundle {}: {error}", path.display()))?;
    let bundle = serde_json::from_str(&content).map_err(|error| {
        format!(
            "identity bundle {} is not valid JSON: {error}",
            path.display()
        )
    })?;

    Ok(Some(bundle))
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

fn detach_owned_daemon_on_exit(lifecycle: &Mutex<DaemonLifecycleManager>) {
    // The daemon deliberately outlives the console window (#207). Closing the
    // console used to kill the network stack underneath every running jolt
    // app; now the daemon keeps running and the next console start reattaches
    // through the health probe. The explicit Stop button still terminates it.
    if let Ok(mut manager) = lifecycle.lock() {
        if let Some(child) = manager.child.take() {
            drop(child);
        }
    }
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

fn safe_identity_export_filename(value: &str) -> String {
    let filename = value
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => character,
            _ => '_',
        })
        .collect::<String>();

    if filename.is_empty() {
        "jolt-identity".to_string()
    } else {
        filename
    }
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
            let lines = content.lines().map(strip_ansi).collect::<Vec<_>>();
            let start = lines.len().saturating_sub(line_count);
            lines[start..].to_vec()
        })
        .unwrap_or_default()
}

// Daemons before 0.5.3 wrote terminal colour codes into the log file; the
// Settings page renders it as plain text, so the escapes are dropped here.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ ... final byte in 0x40..=0x7e
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... terminated by BEL or ESC \
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-byte escapes such as ESC ( B
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
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
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second launch while the window is parked in the tray reopens it
            // instead of starting a second console against the same daemon.
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Linux window managers take the taskbar icon from the window
            // itself; Tauri does not publish _NET_WM_ICON there, so a bare
            // AppImage shows a generic icon without this (#208).
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager;
                if let Some(icon) = app.default_window_icon().cloned() {
                    for window in app.webview_windows().values() {
                        let _ = window.set_icon(icon.clone());
                    }
                }
            }
            install_tray(app)?;
            #[cfg(target_os = "linux")]
            offer_appimage_menu_entry(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window parks the console in the tray with the daemon
            // still running, the Docker Desktop model. Quit in the tray menu is
            // the exit that stops the daemon too.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon_get,
            daemon_post,
            daemon_delete,
            daemon_lifecycle_status,
            daemon_lifecycle_start,
            daemon_lifecycle_stop,
            daemon_lifecycle_restart,
            identity_export_save_file,
            identity_export_open_file,
            console_install_kind
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Jolt Console")
        .run(|app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                detach_owned_daemon_on_exit(&app_handle.state::<Mutex<DaemonLifecycleManager>>());
            }
        });
}

/// How this Console was installed, so the updater is only offered for the
/// bundle types whose update payload we publish (the AppImage on Linux).
#[tauri::command]
fn console_install_kind() -> String {
    use tauri::utils::config::BundleType;
    use tauri::utils::platform::bundle_type;
    match bundle_type() {
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::Msi) | Some(BundleType::Nsis) => "windows",
        Some(BundleType::App) => "macos",
        _ => "unknown",
    }
    .to_string()
}

// Offers, once, to add the AppImage to the applications menu. The dialog is
// modal, so it runs off the main thread after the window is up.
#[cfg(target_os = "linux")]
fn offer_appimage_menu_entry(app: tauri::AppHandle) {
    use desktop_integration as integration;
    let Some(context) = integration::appimage_context() else {
        return;
    };
    let Some(data_home) = integration::data_home() else {
        return;
    };
    let paths = integration::integration_paths(&data_home);
    if integration::integration_state(&paths) != integration::IntegrationState::Offer {
        return;
    }
    std::thread::spawn(move || {
        use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogKind};
        let add = app
            .dialog()
            .message(
                "Add Jolt Console to your applications menu? This writes a menu entry and icon for this AppImage into your home directory, so the panel and app menu show it with the right icon.",
            )
            .title("Jolt Console")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom(
                "Add to menu".to_string(),
                "Not now".to_string(),
            ))
            .blocking_show();
        let result = if add {
            integration::install(&context, &paths)
        } else {
            integration::decline(&paths)
        };
        if let Err(error) = result {
            eprintln!("Jolt Console menu entry: {error}");
        }
    });
}

const TRAY_OPEN: &str = "open-console";
const TRAY_QUIT: &str = "quit-and-stop-daemon";

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let open = MenuItem::with_id(app, TRAY_OPEN, "Open Jolt Console", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT, "Quit and stop daemon", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::with_id("jolt-console")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Jolt Console")
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN => show_main_window(app),
            TRAY_QUIT => quit_console(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

fn show_main_window(app: &tauri::AppHandle) {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn quit_console(app: &tauri::AppHandle) {
    use tauri::Manager;
    let lifecycle = app.state::<Mutex<DaemonLifecycleManager>>();
    if let Err(error) = stop_any_daemon(&lifecycle) {
        eprintln!("Jolt Console quit: daemon was not stopped: {error}");
    }
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        process::{Command, Stdio},
    };

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
    fn exit_detach_without_console_owned_child_is_a_noop() {
        let lifecycle = Mutex::new(DaemonLifecycleManager::default());

        detach_owned_daemon_on_exit(&lifecycle);

        assert!(lifecycle.lock().unwrap().child.is_none());
    }

    #[test]
    fn exit_detach_leaves_console_owned_child_running() {
        // Closing the console must not take the daemon down with it (#207):
        // every jolt app depends on the daemon staying up. The child is
        // released, not killed; a later console start reattaches to it.
        let child = long_running_child();
        let pid = child.id();
        let lifecycle = Mutex::new(DaemonLifecycleManager {
            child: Some(child),
            last_error: None,
            log_path: None,
        });

        detach_owned_daemon_on_exit(&lifecycle);

        assert!(lifecycle.lock().unwrap().child.is_none());
        assert!(
            process_is_running(pid),
            "the daemon must survive the console exiting"
        );

        let _ = Command::new("kill").arg(pid.to_string()).status();
    }

    #[test]
    fn quit_stops_a_console_owned_daemon() {
        // Quit is the one exit that takes the daemon down; closing the window
        // only parks the console in the tray.
        let child = long_running_child();
        let pid = child.id();
        let lifecycle = Mutex::new(DaemonLifecycleManager {
            child: Some(child),
            last_error: None,
            log_path: None,
        });

        stop_any_daemon(&lifecycle).unwrap();

        assert!(lifecycle.lock().unwrap().child.is_none());
        assert!(!process_is_running(pid), "quit must stop the owned daemon");
    }

    #[test]
    fn tail_lines_drops_terminal_colour_codes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        std::fs::write(
            &path,
            "\u{1b}[2m2026-09-05T20:49:06Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2mjolt_network::node\u{1b}[2m:\u{1b}[0m Announcing\n",
        )
        .unwrap();

        assert_eq!(
            tail_lines(&path, 5),
            vec!["2026-09-05T20:49:06Z  INFO jolt_network::node: Announcing"]
        );
    }

    #[test]
    fn tail_lines_returns_the_recent_log_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.log");
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();

        assert_eq!(tail_lines(&path, 2), vec!["two", "three"]);
    }

    #[test]
    fn safe_identity_export_filename_removes_path_chars() {
        assert_eq!(
            safe_identity_export_filename("../alice/key:jolt"),
            ".._alice_key_jolt"
        );
        assert_eq!(safe_identity_export_filename(""), "jolt-identity");
    }

    #[cfg(unix)]
    fn long_running_child() -> std::process::Child {
        Command::new("sleep").arg("30").spawn().unwrap()
    }

    #[cfg(windows)]
    fn long_running_child() -> std::process::Child {
        Command::new("cmd")
            .args(["/C", "timeout /T 30 /NOBREAK >NUL"])
            .spawn()
            .unwrap()
    }

    #[cfg(unix)]
    fn process_is_running(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    #[cfg(windows)]
    fn process_is_running(pid: u32) -> bool {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
    }
}
