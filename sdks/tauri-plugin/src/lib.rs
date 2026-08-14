//! Tauri plugin exposing the local Jolt daemon to app webviews.
//!
//! Jolt desktop applications proxy daemon calls through Rust so the webview
//! never needs direct network access to the daemon. Before this plugin every
//! app copied the same proxy commands into its own `src-tauri` (Spoke and
//! Pastey carried diverged copies); now an app adds one dependency and one
//! line:
//!
//! ```rust,ignore
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_jolt::init())
//!     // ...
//! ```
//!
//! and grants the capability `"jolt:default"` in its capabilities file. The
//! JS side pairs with `@jolt/sdk/transport-tauri`:
//!
//! ```ts
//! new TauriTransport({ plugin: true })
//! ```
//!
//! The daemon base URL defaults to `http://127.0.0.1:9862` and can be
//! overridden with the `JOLT_DAEMON_URL` environment variable. Only the
//! `/app/v1` and `/api/v1` API surfaces are reachable; anything else is
//! rejected before a request is made.

use reqwest::{
    header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE},
    multipart::{Form, Part},
};
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:9862";

#[derive(Debug, Serialize)]
struct DaemonRequestError {
    kind: &'static str,
    message: String,
    status: Option<u16>,
    code: Option<String>,
    body: Option<Value>,
}

impl DaemonRequestError {
    fn api(status: reqwest::StatusCode, message: String, body: Option<Value>) -> Self {
        let code = body
            .as_ref()
            .and_then(|value| value.get("code"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        Self {
            kind: "api",
            message,
            status: Some(status.as_u16()),
            code,
            body,
        }
    }

    fn transport(message: String) -> Self {
        Self {
            kind: "transport",
            message,
            status: None,
            code: None,
            body: None,
        }
    }

    fn configuration(message: String) -> Self {
        Self {
            kind: "configuration",
            message,
            status: None,
            code: None,
            body: None,
        }
    }

    fn invalid_response(message: String, status: reqwest::StatusCode) -> Self {
        Self {
            kind: "invalid_response",
            message,
            status: Some(status.as_u16()),
            code: None,
            body: None,
        }
    }
}

/// Initialize the plugin. Register with `.plugin(tauri_plugin_jolt::init())`.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("jolt")
        .invoke_handler(tauri::generate_handler![
            daemon_request,
            daemon_publish_bytes,
            daemon_append
        ])
        .build()
}

/// Proxy a JSON (or empty-body) request to the daemon.
#[tauri::command]
async fn daemon_request(
    base_path: String,
    path: String,
    method: String,
    body: Option<Value>,
    session_token: Option<String>,
) -> Result<Value, DaemonRequestError> {
    let method = method.parse::<reqwest::Method>().map_err(|error| {
        DaemonRequestError::configuration(format!(
            "invalid daemon request method {method}: {error}"
        ))
    })?;
    let url = daemon_url(&base_path, &path).map_err(DaemonRequestError::configuration)?;
    let client = reqwest::Client::builder()
        .timeout(request_timeout(&base_path, &path))
        .build()
        .map_err(|error| {
            DaemonRequestError::configuration(format!(
                "failed to create daemon HTTP client: {error}"
            ))
        })?;
    let mut request = client
        .request(method, url)
        .header(ACCEPT, "application/json");

    if let Some(token) = session_token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(body) = body {
        request = request.header(CONTENT_TYPE, "application/json").json(&body);
    }

    parse_response(request.send().await).await
}

/// Proxy a multipart publish of raw bytes to `/app/v1/publish`.
#[tauri::command]
async fn daemon_publish_bytes(
    session_token: String,
    path: String,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: String,
) -> Result<Value, DaemonRequestError> {
    multipart_upload("/publish", session_token, path, bytes, file_name, mime_type).await
}

/// Proxy a multipart append-record publish to `/app/v1/append`.
#[tauri::command]
async fn daemon_append(
    session_token: String,
    path: String,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: String,
) -> Result<Value, DaemonRequestError> {
    multipart_upload("/append", session_token, path, bytes, file_name, mime_type).await
}

async fn multipart_upload(
    endpoint: &str,
    session_token: String,
    path: String,
    bytes: Vec<u8>,
    file_name: String,
    mime_type: String,
) -> Result<Value, DaemonRequestError> {
    let file = Part::bytes(bytes)
        .file_name(file_name)
        .mime_str(&mime_type)
        .map_err(|error| {
            DaemonRequestError::configuration(format!("failed to prepare Jolt upload: {error}"))
        })?;
    let form = Form::new().part("file", file).text("path", path);
    let request = reqwest::Client::new()
        .post(daemon_url("/app/v1", endpoint).map_err(DaemonRequestError::configuration)?)
        .header(ACCEPT, "application/json")
        .header(AUTHORIZATION, format!("Bearer {session_token}"))
        .multipart(form);

    parse_response(request.send().await).await
}

async fn parse_response(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<Value, DaemonRequestError> {
    let response = response.map_err(|error| {
        DaemonRequestError::transport(format!("daemon request failed: {error}"))
    })?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = response.text().await.map_err(|error| {
        DaemonRequestError::transport(format!("daemon response read failed: {error}"))
    })?;

    if !status.is_success() {
        let parsed = if content_type.contains("application/json") {
            serde_json::from_str::<Value>(&body).ok()
        } else {
            None
        };
        if content_type.contains("application/json") {
            if let Some(value) = parsed.as_ref() {
                if let Some(error) = value.get("error").and_then(Value::as_str) {
                    return Err(DaemonRequestError::api(status, error.to_string(), parsed));
                }
            }
        }

        let message = if body.trim().is_empty() {
            format!("daemon returned {status}")
        } else {
            body
        };
        return Err(DaemonRequestError::api(status, message, parsed));
    }

    if content_type.contains("application/json") {
        serde_json::from_str(&body).map_err(|error| {
            DaemonRequestError::invalid_response(
                format!("daemon returned invalid JSON: {error}"),
                status,
            )
        })
    } else {
        Ok(Value::String(body))
    }
}

fn daemon_url(base_path: &str, path: &str) -> Result<String, String> {
    let prefix = match base_path {
        "/app/v1" | "/api/v1" => base_path,
        _ => return Err(format!("unsupported daemon base path: {base_path}")),
    };

    Ok(format!(
        "{}{}{}",
        daemon_base_url().trim_end_matches('/'),
        prefix,
        normalize_path(path)
    ))
}

fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn daemon_base_url() -> String {
    std::env::var("JOLT_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string())
}

fn request_timeout(base_path: &str, path: &str) -> Duration {
    // The status endpoint backs "is the daemon up" indicators and must fail
    // fast; everything else gets room for slow network fetches.
    if base_path == "/api/v1" && normalize_path(path) == "/status" {
        Duration::from_secs(3)
    } else {
        Duration::from_secs(60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_request_error_preserves_http_status_for_tauri_transport() {
        let error = DaemonRequestError::api(
            reqwest::StatusCode::NOT_FOUND,
            "missing feature endpoint".to_string(),
            Some(serde_json::json!({ "error": "not found" })),
        );

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "api",
                "message": "missing feature endpoint",
                "status": 404,
                "code": null,
                "body": { "error": "not found" }
            })
        );
    }

    #[test]
    fn daemon_url_accepts_daemon_api_paths() {
        assert_eq!(
            daemon_url("/app/v1", "/published").unwrap(),
            "http://127.0.0.1:9862/app/v1/published"
        );
        assert_eq!(
            daemon_url("/api/v1", "status").unwrap(),
            "http://127.0.0.1:9862/api/v1/status"
        );
    }

    #[test]
    fn daemon_url_rejects_unknown_base_paths() {
        assert_eq!(
            daemon_url("/admin/v1", "/status").unwrap_err(),
            "unsupported daemon base path: /admin/v1"
        );
    }

    #[test]
    fn status_request_uses_short_timeout() {
        assert_eq!(request_timeout("/api/v1", "status"), Duration::from_secs(3));
        assert_eq!(
            request_timeout("/app/v1", "/fetch"),
            Duration::from_secs(60)
        );
    }
}
