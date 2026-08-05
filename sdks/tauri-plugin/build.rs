const COMMANDS: &[&str] = &["daemon_request", "daemon_publish_bytes", "daemon_append"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
