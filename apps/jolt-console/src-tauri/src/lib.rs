#[tauri::command]
async fn daemon_get(path: String) -> Result<serde_json::Value, String> {
    let base_url =
        std::env::var("JOLT_DAEMON_URL").unwrap_or_else(|_| "http://127.0.0.1:9862".to_string());
    let url = format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );

    let response = reqwest::Client::new()
        .get(&url)
        .header("Accept", "application/json")
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![daemon_get])
        .run(tauri::generate_context!())
        .expect("failed to run Jolt Console");
}
