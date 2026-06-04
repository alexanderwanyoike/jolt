use jolt_core::{
    ContentId, JoltAddress, PinRequest, RelayRecord, RelayRecordCapability, UpdateAction,
    UpdateLogEntry,
};
use jolt_identity::NodeIdentity;
use jolt_network::{
    DaemonHandle, HomeRelayCapability, HomeRelayConfig, Multiaddr, NetworkConfig, NetworkNode,
    PeerId,
};
use jolt_store::{CacheConfig, ContentStore};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::sync::mpsc;

static NEXT_TEST_PORT: AtomicU16 = AtomicU16::new(20_000);

/// Helper: spin up a daemon and HTTP server on a random port, return the port and handles.
async fn start_test_server() -> (u16, DaemonHandle, tempfile::TempDir) {
    start_test_server_with_node(None).await
}

async fn start_test_server_with_tcp_port(p2p_port: u16) -> (u16, DaemonHandle, tempfile::TempDir) {
    start_test_server_with_node(Some(p2p_port)).await
}

async fn start_test_server_with_session_path(
    session_path: PathBuf,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
    node.set_fetch_timeout(std::time::Duration::from_secs(2));
    node.set_resolve_timeout(std::time::Duration::from_secs(2));

    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let handle = DaemonHandle::new(cmd_tx);
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });

    let sessions = jolt_server::session_store::AppSessionStore::open(session_path).unwrap();
    let (port, _server_handle) =
        jolt_server::server::start_server_with_session_store(handle.clone(), 0, sessions)
            .await
            .unwrap();

    (port, handle, dir)
}

async fn start_test_server_with_node(
    p2p_port: Option<u16>,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

    if let Some(port) = p2p_port {
        node.listen_on(&format!("/ip4/127.0.0.1/tcp/{port}"))
            .unwrap();
    }

    start_test_server_from_node(node, dir).await
}

async fn start_test_server_from_node(
    mut node: NetworkNode,
    dir: tempfile::TempDir,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    // Use short fetch timeout for tests
    node.set_fetch_timeout(std::time::Duration::from_secs(2));
    node.set_resolve_timeout(std::time::Duration::from_secs(2));

    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let handle = DaemonHandle::new(cmd_tx);

    // Run daemon loop in background
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });

    // Start HTTP server on random port
    let sessions =
        jolt_server::session_store::AppSessionStore::open(dir.path().join("app-sessions.json"))
            .unwrap();
    let (port, _server_handle) =
        jolt_server::server::start_server_with_session_store(handle.clone(), 0, sessions)
            .await
            .unwrap();

    (port, handle, dir)
}

fn signed_profile_log(identity: &NodeIdentity, label: &[u8]) -> Vec<UpdateLogEntry> {
    vec![UpdateLogEntry::genesis(
        identity.public_key_bytes(),
        UpdateAction::SetPath {
            path: "/profile".to_string(),
            content_id: ContentId::from_bytes(label),
        },
        |bytes| identity.sign(bytes),
    )
    .unwrap()]
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

async fn approve_app_session(
    client: &reqwest::Client,
    port: u16,
    identity: &str,
    capabilities: &[&str],
) -> String {
    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": identity,
            "requested_capabilities": capabilities
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": identity,
            "capabilities": capabilities,
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    approved["session_token"].as_str().unwrap().to_string()
}

fn free_tcp_port() -> u16 {
    loop {
        let port = NEXT_TEST_PORT.fetch_add(1, Ordering::Relaxed);
        assert!(port < 30_000, "exhausted test port range");

        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
}

fn no_mdns_config() -> NetworkConfig {
    NetworkConfig {
        enable_mdns: false,
        ..NetworkConfig::test_config()
    }
}

fn relay_config() -> NetworkConfig {
    NetworkConfig {
        bootstrap_relay: true,
        enable_mdns: false,
        ..NetworkConfig::test_config()
    }
}

fn relay_record(identity: &NodeIdentity, observed_at: u64, expires_at: u64) -> RelayRecord {
    RelayRecord::new(
        identity.public_key_bytes(),
        identity.peer_id().to_string(),
        vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
        vec![RelayRecordCapability::Bootstrap],
        observed_at,
        expires_at,
        |bytes| identity.sign(bytes),
    )
    .unwrap()
}

async fn wait_for_connected_peers(handle: &DaemonHandle, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let status = handle.status().await.unwrap();
            if status.connected_peers >= expected {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("timed out waiting for connected peers");
}

#[tokio::test]
async fn test_dashboard_root_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client.get(base_url(port)).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.starts_with("text/html"));

    let body = resp.text().await.unwrap();
    assert!(body.contains("Jolt Node Console"));
    assert!(body.contains("/api/v1/status"));
    assert!(body.contains("/api/v1/publish"));
    assert!(body.contains("/api/v1/published"));
    assert!(body.contains("bootstrap-state"));
    assert!(body.contains("publish-path"));
    assert!(body.contains("fetch-target"));
    assert!(body.contains("/api/v1/peers/connect"));
    assert!(body.contains("/api/v1/resolve"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_dashboard_path_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/dashboard", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Jolt Node Console"));
    assert!(body.contains("/api/v1/cache/entries"));
    assert!(body.contains("Resolve"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_health_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/health", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_session_request_is_visible_to_admin() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "app_origin": "http://127.0.0.1:5174",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "resolve:public",
                "fetch:public",
                "publish:/pastes/*",
                "inventory:/pastes/*",
                "pin:own:/pastes/*"
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();
    assert!(!request_id.is_empty());
    assert_eq!(requested["status"], "pending");

    let admin_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(admin_resp.status(), 200);
    let requests: serde_json::Value = admin_resp.json().await.unwrap();
    let requests = requests.as_array().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["request_id"], request_id);
    assert_eq!(requests[0]["status"], "pending");
    assert_eq!(requests[0]["app_id"], "pastey.local");
    assert_eq!(requests[0]["app_name"], "Pastey");
    assert_eq!(requests[0]["requested_identity"], "alice-public.jolt");
    assert_eq!(
        requests[0]["requested_capabilities"]
            .as_array()
            .unwrap()
            .len(),
        5
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_approve_app_session_request() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "resolve:public",
                "fetch:public",
                "publish:/pastes/*"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": [
                "resolve:public",
                "fetch:public",
                "publish:/pastes/*"
            ],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    let session_id = approved["session_id"].as_str().unwrap();
    let session_token = approved["session_token"].as_str().unwrap();
    assert!(!session_id.is_empty());
    assert!(!session_token.is_empty());
    assert_eq!(approved["request_id"], request_id);
    assert_eq!(approved["status"], "active");
    assert_eq!(approved["identity"], "alice-public.jolt");

    let sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(sessions_resp.status(), 200);
    let sessions: serde_json::Value = sessions_resp.json().await.unwrap();
    let sessions = sessions.as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], session_id);
    assert_eq!(sessions[0]["request_id"], request_id);
    assert_eq!(sessions[0]["status"], "active");
    assert_eq!(sessions[0]["identity"], "alice-public.jolt");
    assert!(sessions[0].get("session_token").is_none());
    assert!(sessions[0].get("token_hash").is_none());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_cannot_approve_forbidden_app_session_capabilities() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "resolve:public",
                "export:keys",
                "delete:identity"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": [
                "resolve:public",
                "export:keys",
                "delete:identity"
            ],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 400);
    let body: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_store_error");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("capability is not grantable"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_cannot_approve_capabilities_beyond_app_request() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "publish:/pastes/*"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": [
                "publish:/drops/*"
            ],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 400);
    let body: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_store_error");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("was not requested"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_approve_narrower_path_scope_than_app_requested() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "publish:/pastes/*"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": [
                "publish:/pastes/public"
            ],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(approved["capabilities"][0], "publish:/pastes/public");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_cannot_approve_malformed_path_scope_capabilities() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": [
                "publish:/pastes*"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": [
                "publish:/pastes*"
            ],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 400);
    let body: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_store_error");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("capability is not grantable"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_poll_approved_session_request() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let pending_resp = client
        .get(format!("{}/app/v1/sessions/{request_id}", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    assert_eq!(pending["status"], "pending");
    assert!(pending["session_token"].is_null());

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved_by_admin: serde_json::Value = approve_resp.json().await.unwrap();

    let approved_resp = client
        .get(format!("{}/app/v1/sessions/{request_id}", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(approved_resp.status(), 200);
    let approved: serde_json::Value = approved_resp.json().await.unwrap();
    assert_eq!(approved["status"], "active");
    assert_eq!(approved["request_id"], request_id);
    assert_eq!(approved["session_id"], approved_by_admin["session_id"]);
    assert_eq!(
        approved["session_token"],
        approved_by_admin["session_token"]
    );
    assert_eq!(approved["identity"], "alice-public.jolt");
    assert_eq!(
        approved["capabilities"].as_array().unwrap(),
        approved_by_admin["capabilities"].as_array().unwrap()
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_reject_app_session_request() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let reject_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/reject",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(reject_resp.status(), 200);
    let rejected: serde_json::Value = reject_resp.json().await.unwrap();
    assert_eq!(rejected["request_id"], request_id);
    assert_eq!(rejected["status"], "rejected");

    let pending_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_revoke_active_app_session() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    let session_id = approved["session_id"].as_str().unwrap();

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/app-sessions/{session_id}/revoke",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(revoke_resp.status(), 200);
    let revoked: serde_json::Value = revoke_resp.json().await.unwrap();
    assert_eq!(revoked["session_id"], session_id);
    assert_eq!(revoked["status"], "revoked");

    let sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(sessions_resp.status(), 200);
    let sessions: serde_json::Value = sessions_resp.json().await.unwrap();
    assert_eq!(sessions.as_array().unwrap().len(), 1);
    assert_eq!(sessions[0]["session_id"], session_id);
    assert_eq!(sessions[0]["status"], "revoked");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_revoked_app_session_token_is_rejected() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    let session_id = approved["session_id"].as_str().unwrap();
    let session_token = approved["session_token"].as_str().unwrap();

    let current_resp = client
        .get(format!("{}/app/v1/session", base_url(port)))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap();
    assert_eq!(current_resp.status(), 200);
    let current: serde_json::Value = current_resp.json().await.unwrap();
    assert_eq!(current["session_id"], session_id);
    assert_eq!(current["status"], "active");
    assert!(current.get("session_token").is_none());
    assert!(current.get("token_hash").is_none());

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/app-sessions/{session_id}/revoke",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);

    let revoked_poll_resp = client
        .get(format!("{}/app/v1/sessions/{request_id}", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(revoked_poll_resp.status(), 200);
    let revoked_poll: serde_json::Value = revoked_poll_resp.json().await.unwrap();
    assert_eq!(revoked_poll["status"], "revoked");
    assert!(revoked_poll["session_token"].is_null());

    let revoked_current_resp = client
        .get(format!("{}/app/v1/session", base_url(port)))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap();
    assert_eq!(revoked_current_resp.status(), 401);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_publish_requires_session_and_path_capability() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &["publish:/pastes/*", "inventory:/pastes/*"],
    )
    .await;
    let wrong_identity_token =
        approve_app_session(&client, port, "wrongidentity.jolt", &["publish:/pastes/*"]).await;

    let unauthorized_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"missing token".to_vec()).file_name("paste.txt"),
        )
        .text("path", "/pastes/one");
    let unauthorized = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .multipart(unauthorized_form)
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), 401);

    let denied_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"outside prefix".to_vec()).file_name("secret.txt"),
        )
        .text("path", "/secrets/one");
    let denied = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .bearer_auth(&token)
        .multipart(denied_form)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let escape_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"path escape".to_vec()).file_name("escape.txt"),
        )
        .text("path", "/pastes/../secrets");
    let escaped = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .bearer_auth(&token)
        .multipart(escape_form)
        .send()
        .await
        .unwrap();
    assert_eq!(escaped.status(), 400);

    let wrong_identity_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"wrong identity".to_vec()).file_name("wrong.txt"),
        )
        .text("path", "/pastes/wrong");
    let wrong_identity = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .bearer_auth(&wrong_identity_token)
        .multipart(wrong_identity_form)
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_identity.status(), 403);

    let allowed_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"app paste".to_vec()).file_name("paste.txt"),
        )
        .text("path", "/pastes/one");
    let allowed = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .bearer_auth(&token)
        .multipart(allowed_form)
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), 200);
    let published: serde_json::Value = allowed.json().await.unwrap();
    assert_eq!(published["path"], "/pastes/one");
    assert_eq!(published["address"], format!("{identity}/pastes/one"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_inventory_is_filtered_to_granted_prefix() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(&client, port, &identity, &["inventory:/pastes/*"]).await;

    for (path, data) in [
        ("/pastes/visible", b"visible paste".to_vec()),
        ("/notes/hidden", b"hidden note".to_vec()),
    ] {
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(data).file_name("item.txt"),
            )
            .text("path", path.to_string());
        let publish_resp = client
            .post(format!("{}/api/v1/publish", base_url(port)))
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(publish_resp.status(), 200);
    }

    let inventory_resp = client
        .get(format!("{}/app/v1/published", base_url(port)))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(inventory_resp.status(), 200);
    let inventory: serde_json::Value = inventory_resp.json().await.unwrap();
    let items = inventory.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["path"], "/pastes/visible");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_resolve_and_fetch_require_public_capabilities() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"public paste".to_vec()).file_name("paste.txt"),
        )
        .text("path", "/pastes/public");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let address = published["address"].as_str().unwrap();
    let content_id = published["content_id"].as_str().unwrap();

    let resolve_only = approve_app_session(&client, port, &identity, &["resolve:public"]).await;
    let fetch_only = approve_app_session(&client, port, &identity, &["fetch:public"]).await;

    let denied_resolve = client
        .post(format!("{}/app/v1/resolve", base_url(port)))
        .bearer_auth(&fetch_only)
        .json(&serde_json::json!({ "address": address }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_resolve.status(), 403);

    let resolved_resp = client
        .post(format!("{}/app/v1/resolve", base_url(port)))
        .bearer_auth(&resolve_only)
        .json(&serde_json::json!({ "address": address }))
        .send()
        .await
        .unwrap();
    assert_eq!(resolved_resp.status(), 200);
    let resolved: serde_json::Value = resolved_resp.json().await.unwrap();
    assert_eq!(resolved["content_id"], content_id);

    let denied_fetch = client
        .post(format!("{}/app/v1/fetch", base_url(port)))
        .bearer_auth(&resolve_only)
        .json(&serde_json::json!({ "target": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_fetch.status(), 403);

    let fetched_resp = client
        .post(format!("{}/app/v1/fetch", base_url(port)))
        .bearer_auth(&fetch_only)
        .json(&serde_json::json!({ "target": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(fetched_resp.status(), 200);
    let fetched: serde_json::Value = fetched_resp.json().await.unwrap();
    assert_eq!(fetched["content_id"], content_id);
    let data: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(data, b"public paste");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_pin_requires_own_published_content_in_granted_prefix() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(&client, port, &identity, &["pin:own:/pastes/*"]).await;

    let hidden_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"hidden".to_vec()).file_name("hidden.txt"),
        )
        .text("path", "/notes/hidden");
    let hidden_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(hidden_form)
        .send()
        .await
        .unwrap();
    assert_eq!(hidden_resp.status(), 200);
    let hidden: serde_json::Value = hidden_resp.json().await.unwrap();
    let hidden_content_id = hidden["content_id"].as_str().unwrap();

    let denied = client
        .post(format!("{}/app/v1/home-relay/pins", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "content_id": hidden_content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let visible_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"visible".to_vec()).file_name("visible.txt"),
        )
        .text("path", "/pastes/visible");
    let visible_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(visible_form)
        .send()
        .await
        .unwrap();
    assert_eq!(visible_resp.status(), 200);
    let visible: serde_json::Value = visible_resp.json().await.unwrap();
    let visible_content_id = visible["content_id"].as_str().unwrap();

    let authorized_without_home_relay = client
        .post(format!("{}/app/v1/home-relay/pins", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "content_id": visible_content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorized_without_home_relay.status(), 400);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_sessions_persist_across_server_restart() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (first_port, first_handle, _first_dir) =
        start_test_server_with_session_path(session_path.clone()).await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(first_port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "alice-public.jolt",
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(first_port)
        ))
        .json(&serde_json::json!({
            "identity": "alice-public.jolt",
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    let session_id = approved["session_id"].as_str().unwrap().to_string();
    assert!(!approved["session_token"].as_str().unwrap().is_empty());

    first_handle.shutdown().await.ok();

    let (second_port, second_handle, _second_dir) =
        start_test_server_with_session_path(session_path).await;
    let sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(second_port)))
        .send()
        .await
        .unwrap();

    assert_eq!(sessions_resp.status(), 200);
    let sessions: serde_json::Value = sessions_resp.json().await.unwrap();
    let sessions = sessions.as_array().unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["session_id"], session_id);
    assert_eq!(sessions[0]["status"], "active");
    assert_eq!(sessions[0]["identity"], "alice-public.jolt");
    assert!(sessions[0].get("session_token").is_none());
    assert!(sessions[0].get("token_hash").is_none());

    second_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["peer_id"].is_string());
    assert!(!body["peer_id"].as_str().unwrap().is_empty());
    assert!(body["identity_address"].is_string());
    let identity_address = body["identity_address"].as_str().unwrap();
    assert!(identity_address.ends_with(".jolt"));
    assert!(body["uptime_secs"].is_number());
    assert_eq!(body["connected_peers"], 0);
    assert_eq!(body["bootstrap_relay"], false);
    assert_eq!(
        body["configured_bootstrap_relays"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        body["effective_bootstrap_relays"].as_array().unwrap().len(),
        0
    );
    assert_eq!(body["bootstrap_state"], "disconnected");
    assert_eq!(body["configured_bootstrap_relay_count"], 0);
    assert_eq!(body["effective_bootstrap_relay_count"], 0);
    assert_eq!(body["connected_bootstrap_peers"], 0);
    assert!(body["last_bootstrap_error"].is_null());
    assert!(body["home_relay"].is_null());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint_reports_home_relay_config() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut config = NetworkConfig::test_config();
    config.home_relay = Some(jolt_network::HomeRelayConfig {
        peer_id: "12D3HomeRelay".to_string(),
        multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3HomeRelay".to_string(),
        capability: jolt_network::HomeRelayCapability::Pinning,
        api_url: Some("http://127.0.0.1:9862".to_string()),
    });
    let node = NetworkNode::new_tcp(identity, store, config).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["home_relay"]["multiaddr"],
        "/ip4/127.0.0.1/tcp/4001/p2p/12D3HomeRelay"
    );
    assert_eq!(body["home_relay"]["capability"], "pinning");
    assert_eq!(body["home_relay"]["api_url"], "http://127.0.0.1:9862");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint_reports_relay_record_in_relay_mode() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(identity, store, relay_config()).unwrap();
    let p2p_port = free_tcp_port();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{p2p_port}"))
        .unwrap();
    let (port, handle, _dir) = start_test_server_from_node(relay, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["bootstrap_relay"], true);
    assert_eq!(body["relay_record"]["body"]["peer_id"], body["peer_id"]);
    assert_eq!(
        body["relay_record"]["body"]["capabilities"],
        serde_json::json!(["Bootstrap", "Discovery", "Pinning"])
    );
    assert!(body["relay_record"]["body"]["addrs"][0]
        .as_str()
        .unwrap()
        .contains(&format!("/tcp/{p2p_port}")));
    assert!(body["relay_record"]["signature"].is_array());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint_reports_known_relay_count() {
    let dir = tempfile::tempdir().unwrap();
    let relay_identity = NodeIdentity::generate();
    let node_identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    store
        .record_relay_record(relay_record(&relay_identity, 100, 4_102_444_800), 110)
        .unwrap();
    let node = NetworkNode::new_tcp(node_identity, store, NetworkConfig::test_config()).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["known_relay_count"], 1);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint_reports_connected_bootstrap_peer() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (_relay_api, relay_handle, _relay_dir) =
        start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");
    let relay_addr: Multiaddr = relay_multiaddr.parse().unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.effective_bootstrap_relays = vec![relay_multiaddr.clone()];
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr]).unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;
    wait_for_connected_peers(&alice_handle, 1).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/v1/status", base_url(alice_api)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["bootstrap_state"], "connected");
    assert_eq!(body["effective_bootstrap_relay_count"], 1);
    assert_eq!(body["connected_bootstrap_peers"], 1);
    assert!(body["last_bootstrap_error"].is_null());

    alice_handle.shutdown().await.ok();
    relay_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_peers_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/peers", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.is_array());
    assert_eq!(body.as_array().unwrap().len(), 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_uses_verified_update_log_cache() {
    let dir = tempfile::tempdir().unwrap();
    let owner = NodeIdentity::generate();
    let identity_id = owner.identity_id();
    let address = JoltAddress::new(identity_id.clone(), "/profile").unwrap();
    let content_id = ContentId::from_bytes(b"cached profile");
    let update_log = signed_profile_log(&owner, b"cached profile");
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(owner, store, NetworkConfig::test_config()).unwrap();
    node.store_verified_update_log(identity_id.clone(), update_log)
        .unwrap();

    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({ "address": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["address"], address.to_string());
    assert_eq!(body["identity"], identity_id.to_string());
    assert_eq!(body["path"], "/profile");
    assert_eq!(body["latest_sequence"], 0);
    assert_eq!(body["content_id"], content_id.to_string());
    assert_eq!(body["reachability_hints"].as_array().unwrap().len(), 0);
    assert_eq!(body["source"], "cache");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_rejects_malformed_jolt_address() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({ "address": "alice" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains(".jolt"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_discovers_update_log_provider_through_bootstrap_relay() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();
    let bob_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, no_mdns_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (_relay_api, relay_handle, _relay_dir) =
        start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}")
        .parse()
        .unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_identity_id = alice_identity.identity_id();
    let address = JoltAddress::new(alice_identity_id.clone(), "/profile").unwrap();
    let content_id = ContentId::from_bytes(b"alice profile via dht resolve");
    let update_log = signed_profile_log(&alice_identity, b"alice profile via dht resolve");
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, no_mdns_config()).unwrap();
    alice
        .store_verified_update_log(alice_identity_id.clone(), update_log)
        .unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    alice
        .announce_update_log_provider(&alice_identity_id)
        .unwrap();
    let (_alice_api, alice_handle, _alice_dir) =
        start_test_server_from_node(alice, alice_dir).await;

    let bob_identity = NodeIdentity::generate();
    let bob_store = ContentStore::open(bob_dir.path(), CacheConfig::default()).unwrap();
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on(&format!("/ip4/127.0.0.1/tcp/{bob_p2p}"))
        .unwrap();
    bob.bootstrap_dht(&[relay_addr]).unwrap();
    let (bob_api, bob_handle, _bob_dir) = start_test_server_from_node(bob, bob_dir).await;

    wait_for_connected_peers(&alice_handle, 1).await;
    wait_for_connected_peers(&bob_handle, 1).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(bob_api)))
        .json(&serde_json::json!({ "address": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["address"], address.to_string());
    assert_eq!(body["identity"], alice_identity_id.to_string());
    assert_eq!(body["path"], "/profile");
    assert_eq!(body["latest_sequence"], 0);
    assert_eq!(body["content_id"], content_id.to_string());
    assert_eq!(body["source"], "network");

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
    bob_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_discovers_path_published_through_http() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();
    let bob_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, no_mdns_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (_relay_api, relay_handle, _relay_dir) =
        start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}")
        .parse()
        .unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_identity_id = alice_identity.identity_id();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, no_mdns_config()).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let bob_identity = NodeIdentity::generate();
    let bob_store = ContentStore::open(bob_dir.path(), CacheConfig::default()).unwrap();
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on(&format!("/ip4/127.0.0.1/tcp/{bob_p2p}"))
        .unwrap();
    bob.bootstrap_dht(&[relay_addr]).unwrap();
    let (bob_api, bob_handle, _bob_dir) = start_test_server_from_node(bob, bob_dir).await;

    wait_for_connected_peers(&alice_handle, 1).await;
    wait_for_connected_peers(&bob_handle, 1).await;

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"network signed path".to_vec()).file_name("hello.txt"),
        )
        .text("path", "/hello");

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let address = published["address"].as_str().unwrap();
    let content_id = ContentId::from_bytes(b"network signed path");

    assert!(address.starts_with(&alice_identity_id.to_string()));
    assert!(address.ends_with(".jolt/hello"));

    let resolve_resp = client
        .post(format!("{}/api/v1/resolve", base_url(bob_api)))
        .json(&serde_json::json!({ "address": address }))
        .send()
        .await
        .unwrap();

    assert_eq!(resolve_resp.status(), 200);
    let resolved: serde_json::Value = resolve_resp.json().await.unwrap();
    assert_eq!(resolved["address"], address);
    assert_eq!(resolved["identity"], alice_identity_id.to_string());
    assert_eq!(resolved["path"], "/hello");
    assert_eq!(resolved["content_id"], content_id.to_string());
    assert_eq!(resolved["source"], "network");

    let fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(bob_api)))
        .json(&serde_json::json!({ "target": address }))
        .send()
        .await
        .unwrap();

    assert_eq!(fetch_resp.status(), 200);
    let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
    assert_eq!(fetched["content_id"], content_id.to_string());
    assert_eq!(fetched["size"], 19);
    let fetched_data: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(fetched_data, b"network signed path");

    let bob_status = bob_handle.status().await.unwrap();
    assert_eq!(bob_status.cached_count, 1);

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
    bob_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_existing_peer_resolves_path_published_after_owner_restart() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();
    let restarted_alice_p2p = free_tcp_port();
    let bob_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, no_mdns_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (_relay_api, relay_handle, _relay_dir) =
        start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}")
        .parse()
        .unwrap();

    let alice_identity = NodeIdentity::generate();
    alice_identity.save(alice_dir.path()).unwrap();
    let alice_identity_id = alice_identity.identity_id();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, no_mdns_config()).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    let (alice_api, alice_handle, alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let bob_identity = NodeIdentity::generate();
    let bob_store = ContentStore::open(bob_dir.path(), CacheConfig::default()).unwrap();
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on(&format!("/ip4/127.0.0.1/tcp/{bob_p2p}"))
        .unwrap();
    bob.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    let (bob_api, bob_handle, _bob_dir) = start_test_server_from_node(bob, bob_dir).await;

    wait_for_connected_peers(&alice_handle, 1).await;
    wait_for_connected_peers(&bob_handle, 1).await;

    let client = reqwest::Client::new();
    let first_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"first path before restart".to_vec())
                .file_name("first.txt"),
        )
        .text("path", "/first");
    let first_publish = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(first_form)
        .send()
        .await
        .unwrap();
    assert_eq!(first_publish.status(), 200);
    let first_published: serde_json::Value = first_publish.json().await.unwrap();
    let first_address = first_published["address"].as_str().unwrap().to_string();
    assert_eq!(first_published["latest_sequence"], 0);

    let first_resolve = client
        .post(format!("{}/api/v1/resolve", base_url(bob_api)))
        .json(&serde_json::json!({ "address": first_address }))
        .send()
        .await
        .unwrap();
    assert_eq!(first_resolve.status(), 200);

    alice_handle.shutdown().await.ok();

    let restarted_alice_identity = NodeIdentity::load(alice_dir.path()).unwrap();
    assert_eq!(restarted_alice_identity.identity_id(), alice_identity_id);
    let restarted_alice_store =
        ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut restarted_alice = NetworkNode::new_tcp(
        restarted_alice_identity,
        restarted_alice_store,
        no_mdns_config(),
    )
    .unwrap();
    restarted_alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{restarted_alice_p2p}"))
        .unwrap();
    restarted_alice.bootstrap_dht(&[relay_addr]).unwrap();
    let (restarted_alice_api, restarted_alice_handle, _alice_dir) =
        start_test_server_from_node(restarted_alice, alice_dir).await;
    wait_for_connected_peers(&restarted_alice_handle, 1).await;

    let second_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"second path after restart".to_vec())
                .file_name("second.txt"),
        )
        .text("path", "/second");
    let second_publish = client
        .post(format!("{}/api/v1/publish", base_url(restarted_alice_api)))
        .multipart(second_form)
        .send()
        .await
        .unwrap();
    assert_eq!(second_publish.status(), 200);
    let second_published: serde_json::Value = second_publish.json().await.unwrap();
    let second_address = second_published["address"].as_str().unwrap();
    assert!(second_address.starts_with(&alice_identity_id.to_string()));
    assert_eq!(second_published["latest_sequence"], 1);

    let second_resolve = client
        .post(format!("{}/api/v1/resolve", base_url(bob_api)))
        .json(&serde_json::json!({ "address": second_address }))
        .send()
        .await
        .unwrap();
    assert_eq!(second_resolve.status(), 200);
    let resolved: serde_json::Value = second_resolve.json().await.unwrap();
    assert_eq!(resolved["identity"], alice_identity_id.to_string());
    assert_eq!(resolved["path"], "/second");
    assert_eq!(resolved["latest_sequence"], 1);
    assert_eq!(
        resolved["content_id"],
        ContentId::from_bytes(b"second path after restart").to_string()
    );

    relay_handle.shutdown().await.ok();
    restarted_alice_handle.shutdown().await.ok();
    bob_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_offline_publisher_content_is_resolved_and_fetched_through_relay() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let alice_identity_dir = alice_dir.path().to_path_buf();
    let bob_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();
    let bob_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}")
        .parse()
        .unwrap();

    let alice_identity = NodeIdentity::generate();
    alice_identity.save(alice_dir.path()).unwrap();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, no_mdns_config()).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    wait_for_connected_peers(&alice_handle, 1).await;

    let client = reqwest::Client::new();
    let original_data = b"relay pinned content survives alice leaving";
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(original_data.to_vec()).file_name("pinned.txt"),
        )
        .text("path", "/space/post");

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = published["content_id"].as_str().unwrap();
    let address = published["address"].as_str().unwrap();

    let owner = NodeIdentity::load(&alice_identity_dir).unwrap();
    let pin_request = PinRequest::new(
        owner.public_key_bytes(),
        content_id.parse().unwrap(),
        |bytes| owner.sign(bytes),
    )
    .unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/relay/pins", base_url(relay_api)))
        .json(&pin_request)
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);

    let relay_entries = relay_handle.list_cache_entries().await.unwrap();
    assert!(relay_entries
        .iter()
        .any(|entry| entry.content_id == content_id && entry.pinned));

    alice_handle.shutdown().await.ok();

    let bob_identity = NodeIdentity::generate();
    let bob_store = ContentStore::open(bob_dir.path(), CacheConfig::default()).unwrap();
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on(&format!("/ip4/127.0.0.1/tcp/{bob_p2p}"))
        .unwrap();
    bob.bootstrap_dht(&[relay_addr]).unwrap();
    let (bob_api, bob_handle, _bob_dir) = start_test_server_from_node(bob, bob_dir).await;
    wait_for_connected_peers(&bob_handle, 1).await;

    let fetch_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let fetched = loop {
        let fetch_resp = client
            .post(format!("{}/api/v1/fetch", base_url(bob_api)))
            .json(&serde_json::json!({ "target": address }))
            .send()
            .await
            .unwrap();
        let fetch_status = fetch_resp.status();
        let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
        if fetch_status == 200 {
            break fetched;
        }
        assert!(
            std::time::Instant::now() < fetch_deadline,
            "offline .jolt fetch did not converge: {fetched}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert_eq!(fetched["content_id"], content_id);
    let fetched_data: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(fetched_data, original_data);

    relay_handle.shutdown().await.ok();
    bob_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_pin_endpoint_pins_published_content_for_offline_fetch() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();
    let bob_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");
    let relay_addr: Multiaddr = relay_multiaddr.parse().unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.home_relay = Some(HomeRelayConfig {
        peer_id: relay_peer,
        multiaddr: relay_multiaddr,
        capability: HomeRelayCapability::Pinning,
        api_url: Some(base_url(relay_api)),
    });
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let client = reqwest::Client::new();
    let original_data = b"home relay pin endpoint makes alice content durable";
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(original_data.to_vec()).file_name("home-pin.txt"),
        )
        .text("path", "/space/home-pin");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = published["content_id"].as_str().unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);
    let pinned: serde_json::Value = pin_resp.json().await.unwrap();
    assert_eq!(pinned["status"], "pinned");
    assert_eq!(pinned["content_id"], content_id);
    assert_eq!(pinned["latest_sequence"], 0);

    let relay_entries = relay_handle.list_cache_entries().await.unwrap();
    assert!(relay_entries
        .iter()
        .any(|entry| entry.content_id == content_id && entry.pinned));

    let second_data = b"home relay pin endpoint refreshes the owner update log";
    let second_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(second_data.to_vec()).file_name("home-pin-2.txt"),
        )
        .text("path", "/space/home-pin-2");
    let second_publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(second_form)
        .send()
        .await
        .unwrap();
    assert_eq!(second_publish_resp.status(), 200);
    let second_published: serde_json::Value = second_publish_resp.json().await.unwrap();
    let second_content_id = second_published["content_id"].as_str().unwrap();
    let second_address = second_published["address"].as_str().unwrap();

    let second_pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": second_content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(second_pin_resp.status(), 200);
    let second_pinned: serde_json::Value = second_pin_resp.json().await.unwrap();
    assert_eq!(second_pinned["status"], "pinned");
    assert_eq!(second_pinned["content_id"], second_content_id);
    assert_eq!(second_pinned["latest_sequence"], 1);

    let relay_entries = relay_handle.list_cache_entries().await.unwrap();
    assert!(relay_entries
        .iter()
        .any(|entry| entry.content_id == second_content_id && entry.pinned));

    alice_handle.shutdown().await.ok();

    let bob_identity = NodeIdentity::generate();
    let bob_store = ContentStore::open(bob_dir.path(), CacheConfig::default()).unwrap();
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on(&format!("/ip4/127.0.0.1/tcp/{bob_p2p}"))
        .unwrap();
    bob.bootstrap_dht(&[relay_addr]).unwrap();
    let (bob_api, bob_handle, _bob_dir) = start_test_server_from_node(bob, bob_dir).await;
    wait_for_connected_peers(&bob_handle, 1).await;

    let fetch_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let fetched = loop {
        let fetch_resp = client
            .post(format!("{}/api/v1/fetch", base_url(bob_api)))
            .json(&serde_json::json!({ "target": second_address }))
            .send()
            .await
            .unwrap();
        let fetch_status = fetch_resp.status();
        let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
        if fetch_status == 200 {
            break fetched;
        }
        assert!(
            std::time::Instant::now() < fetch_deadline,
            "home-relay pinned .jolt fetch did not converge: {fetched}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert_eq!(fetched["content_id"], second_content_id);
    let fetched_data: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(fetched_data, second_data);

    relay_handle.shutdown().await.ok();
    bob_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_published_inventory_tracks_relay_backed_and_stale_path_state() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.home_relay = Some(HomeRelayConfig {
        peer_id: relay_peer.clone(),
        multiaddr: relay_multiaddr.clone(),
        capability: HomeRelayCapability::Pinning,
        api_url: Some(base_url(relay_api)),
    });
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let client = reqwest::Client::new();
    let path = "/space/stale";
    let first_data = b"first relay-backed version";
    let first_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(first_data.to_vec()).file_name("first.txt"),
        )
        .text("path", path.to_string());
    let first_publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(first_form)
        .send()
        .await
        .unwrap();
    assert_eq!(first_publish_resp.status(), 200);
    let first_published: serde_json::Value = first_publish_resp.json().await.unwrap();
    let first_content_id = first_published["content_id"].as_str().unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": first_content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);

    let inventory_resp = client
        .get(format!("{}/api/v1/published", base_url(alice_api)))
        .send()
        .await
        .unwrap();
    assert_eq!(inventory_resp.status(), 200);
    let inventory: Vec<serde_json::Value> = inventory_resp.json().await.unwrap();
    let item = inventory
        .iter()
        .find(|item| item["path"] == path)
        .expect("path row missing after pin");
    assert_eq!(item["content_id"], first_content_id);
    assert_eq!(item["local_sequence"], 0);
    assert_eq!(item["pin_state"], "relay_backed");
    assert_eq!(item["relay"]["peer_id"], relay_peer);
    assert_eq!(item["relay"]["multiaddr"], relay_multiaddr);
    assert_eq!(item["pinned_content_id"], first_content_id);
    assert_eq!(item["pinned_sequence"], 0);

    let shared_path = "/space/shared";
    let shared_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(first_data.to_vec()).file_name("shared.txt"),
        )
        .text("path", shared_path.to_string());
    let shared_publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(shared_form)
        .send()
        .await
        .unwrap();
    assert_eq!(shared_publish_resp.status(), 200);
    let shared_published: serde_json::Value = shared_publish_resp.json().await.unwrap();
    assert_eq!(shared_published["content_id"], first_content_id);

    let shared_pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": first_content_id, "path": shared_path }))
        .send()
        .await
        .unwrap();
    assert_eq!(shared_pin_resp.status(), 200);

    let second_data = b"second local only version";
    let second_form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(second_data.to_vec()).file_name("second.txt"),
        )
        .text("path", path.to_string());
    let second_publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(second_form)
        .send()
        .await
        .unwrap();
    assert_eq!(second_publish_resp.status(), 200);
    let second_published: serde_json::Value = second_publish_resp.json().await.unwrap();
    let second_content_id = second_published["content_id"].as_str().unwrap();

    let inventory_resp = client
        .get(format!("{}/api/v1/published", base_url(alice_api)))
        .send()
        .await
        .unwrap();
    assert_eq!(inventory_resp.status(), 200);
    let inventory: Vec<serde_json::Value> = inventory_resp.json().await.unwrap();
    let item = inventory
        .iter()
        .find(|item| item["path"] == path)
        .expect("path row missing after update");
    assert_eq!(item["content_id"], second_content_id);
    assert_eq!(item["local_sequence"], 2);
    assert_eq!(item["pin_state"], "needs_repin");
    assert_eq!(item["pinned_content_id"], first_content_id);
    assert_eq!(item["pinned_sequence"], 0);

    let shared_item = inventory
        .iter()
        .find(|item| item["path"] == shared_path)
        .expect("shared path row missing after update");
    assert_eq!(shared_item["content_id"], first_content_id);
    assert_eq!(shared_item["pin_state"], "relay_backed");

    let second_pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": second_content_id, "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(second_pin_resp.status(), 200);

    let inventory_resp = client
        .get(format!("{}/api/v1/published", base_url(alice_api)))
        .send()
        .await
        .unwrap();
    assert_eq!(inventory_resp.status(), 200);
    let inventory: Vec<serde_json::Value> = inventory_resp.json().await.unwrap();
    let item = inventory
        .iter()
        .find(|item| item["path"] == path)
        .expect("path row missing after repin");
    assert_eq!(item["content_id"], second_content_id);
    assert_eq!(item["local_sequence"], 2);
    assert_eq!(item["pin_state"], "relay_backed");
    assert_eq!(item["pinned_content_id"], second_content_id);
    assert_eq!(item["pinned_sequence"], 2);

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_availability_reports_healthy_pin() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.home_relay = Some(HomeRelayConfig {
        peer_id: relay_peer.clone(),
        multiaddr: relay_multiaddr,
        capability: HomeRelayCapability::Pinning,
        api_url: Some(base_url(relay_api)),
    });
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"healthy home relay pin".to_vec())
                .file_name("healthy.txt"),
        )
        .text("path", "/space/healthy");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = published["content_id"].as_str().unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);

    let availability_resp = client
        .get(format!(
            "{}/api/v1/home-relay/availability",
            base_url(alice_api)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(availability_resp.status(), 200);
    let availability: serde_json::Value = availability_resp.json().await.unwrap();
    assert_eq!(availability["status"], "healthy");
    assert_eq!(availability["checked_count"], 1);
    assert_eq!(availability["degraded_count"], 0);
    assert_eq!(availability["items"][0]["content_id"], content_id);
    assert_eq!(availability["items"][0]["path"], "/space/healthy");
    assert_eq!(availability["items"][0]["status"], "available");
    assert_eq!(availability["items"][0]["relay"]["peer_id"], relay_peer);

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_availability_reports_missing_pin() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.home_relay = Some(HomeRelayConfig {
        peer_id: relay_peer,
        multiaddr: relay_multiaddr,
        capability: HomeRelayCapability::Pinning,
        api_url: Some(base_url(relay_api)),
    });
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"missing relay pin".to_vec()).file_name("missing.txt"),
        )
        .text("path", "/space/missing");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = published["content_id"].as_str().unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);

    let unpin_resp = client
        .delete(format!(
            "{}/api/v1/cache/pin/{content_id}",
            base_url(relay_api)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(unpin_resp.status(), 200);

    let availability_resp = client
        .get(format!(
            "{}/api/v1/home-relay/availability",
            base_url(alice_api)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(availability_resp.status(), 200);
    let availability: serde_json::Value = availability_resp.json().await.unwrap();
    assert_eq!(availability["status"], "degraded");
    assert_eq!(availability["checked_count"], 1);
    assert_eq!(availability["degraded_count"], 1);
    assert_eq!(availability["items"][0]["content_id"], content_id);
    assert_eq!(availability["items"][0]["status"], "missing_pin");

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_availability_reports_unreachable_relay_without_breaking_local_content() {
    let relay_dir = tempfile::tempdir().unwrap();
    let alice_dir = tempfile::tempdir().unwrap();
    let relay_p2p = free_tcp_port();
    let alice_p2p = free_tcp_port();

    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, relay_config()).unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

    let alice_identity = NodeIdentity::generate();
    let alice_store = ContentStore::open(alice_dir.path(), CacheConfig::default()).unwrap();
    let mut alice_config = no_mdns_config();
    alice_config.home_relay = Some(HomeRelayConfig {
        peer_id: relay_peer,
        multiaddr: relay_multiaddr,
        capability: HomeRelayCapability::Pinning,
        api_url: Some(base_url(relay_api)),
    });
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"relay will disappear".to_vec())
                .file_name("unreachable.txt"),
        )
        .text("path", "/space/unreachable");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = published["content_id"].as_str().unwrap();

    let pin_resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(pin_resp.status(), 200);

    relay_handle.shutdown().await.ok();

    let availability_resp = client
        .get(format!(
            "{}/api/v1/home-relay/availability",
            base_url(alice_api)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(availability_resp.status(), 200);
    let availability: serde_json::Value = availability_resp.json().await.unwrap();
    assert_eq!(availability["status"], "degraded");
    assert_eq!(availability["checked_count"], 1);
    assert_eq!(availability["degraded_count"], 1);
    assert_eq!(availability["items"][0]["status"], "relay_unreachable");
    assert!(availability["items"][0]["error"]
        .as_str()
        .unwrap()
        .contains("home relay"));

    let local_form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"local publishing still works".to_vec())
            .file_name("local.txt"),
    );
    let local_publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(alice_api)))
        .multipart(local_form)
        .send()
        .await
        .unwrap();
    assert_eq!(local_publish_resp.status(), 200);
    let local_published: serde_json::Value = local_publish_resp.json().await.unwrap();
    let local_content_id = local_published["content_id"].as_str().unwrap();
    let local_fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(alice_api)))
        .json(&serde_json::json!({ "content_id": local_content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(local_fetch_resp.status(), 200);
    let fetched: serde_json::Value = local_fetch_resp.json().await.unwrap();
    assert_eq!(fetched["content_id"], local_content_id);

    alice_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_pin_endpoint_reports_missing_home_relay() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(port)))
        .json(&serde_json::json!({ "content_id": ContentId::from_bytes(b"missing") }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("home relay is not configured"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_pin_endpoint_requires_pinning_capability() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut config = no_mdns_config();
    config.home_relay = Some(HomeRelayConfig {
        peer_id: "12D3HomeRelay".to_string(),
        multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3HomeRelay".to_string(),
        capability: HomeRelayCapability::DiscoveryOnly,
        api_url: None,
    });
    let node = NetworkNode::new_tcp(identity, store, config).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(port)))
        .json(&serde_json::json!({ "content_id": ContentId::from_bytes(b"missing") }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("home relay is not pin-capable"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_home_relay_pin_endpoint_requires_relay_api_url() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut config = no_mdns_config();
    config.home_relay = Some(HomeRelayConfig {
        peer_id: "12D3HomeRelay".to_string(),
        multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3HomeRelay".to_string(),
        capability: HomeRelayCapability::Pinning,
        api_url: None,
    });
    let node = NetworkNode::new_tcp(identity, store, config).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/home-relay/pins", base_url(port)))
        .json(&serde_json::json!({ "content_id": ContentId::from_bytes(b"missing") }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("home relay API URL is not configured"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_relay_pin_request_rejects_tampered_owner_signature() {
    let dir = tempfile::tempdir().unwrap();
    let relay_identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let node = NetworkNode::new_tcp(relay_identity, store, relay_config()).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;

    let owner = NodeIdentity::generate();
    let mut request = PinRequest::new(
        owner.public_key_bytes(),
        ContentId::from_bytes(b"owner intended content"),
        |bytes| owner.sign(bytes),
    )
    .unwrap();
    request.body.content_id = ContentId::from_bytes(b"tampered content");

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/relay/pins", base_url(port)))
        .json(&request)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("invalid signature"));
    let stats = handle.cache_stats().await.unwrap();
    assert_eq!(stats.pinned_items, 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_relay_pin_request_requires_relay_mode() {
    let (port, handle, _dir) = start_test_server().await;
    let owner = NodeIdentity::generate();
    let request = PinRequest::new(
        owner.public_key_bytes(),
        ContentId::from_bytes(b"content ordinary node should not pin"),
        |bytes| owner.sign(bytes),
    )
    .unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/relay/pins", base_url(port)))
        .json(&request)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("not configured"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_reports_no_update_log_provider_candidates() {
    let (port, handle, _dir) = start_test_server().await;
    let missing_identity = NodeIdentity::generate().identity_id();
    let address = JoltAddress::new(missing_identity, "/profile").unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({ "address": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "no_bootstrap_relays");
    assert!(body["error"].as_str().unwrap().contains("jolt:update-log"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_reports_relay_unreachable() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let unreachable_peer = PeerId::random();
    let config = NetworkConfig {
        enable_mdns: false,
        effective_bootstrap_relays: vec![format!("/ip4/127.0.0.1/tcp/9/p2p/{unreachable_peer}")],
        ..NetworkConfig::test_config()
    };
    let node = NetworkNode::new_tcp(identity, store, config).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let missing_identity = NodeIdentity::generate().identity_id();
    let address = JoltAddress::new(missing_identity, "/profile").unwrap();
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({ "address": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "relay_unreachable");
    assert!(body["error"].as_str().unwrap().contains("bootstrap relays"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_reports_identity_provider_not_found_on_reachable_mesh() {
    let p2p_a = free_tcp_port();
    let (_port_a, handle_a, _dir_a) = start_test_server_with_tcp_port(p2p_a).await;
    let (port_b, handle_b, _dir_b) = start_test_server_with_tcp_port(free_tcp_port()).await;
    let client = reqwest::Client::new();

    let peer_a = handle_a.status().await.unwrap().peer_id;
    let multiaddr = format!("/ip4/127.0.0.1/tcp/{p2p_a}/p2p/{peer_a}");
    let connect_resp = client
        .post(format!("{}/api/v1/peers/connect", base_url(port_b)))
        .json(&serde_json::json!({ "multiaddr": multiaddr }))
        .send()
        .await
        .unwrap();
    assert_eq!(connect_resp.status(), 200);
    wait_for_connected_peers(&handle_b, 1).await;

    let missing_identity = NodeIdentity::generate().identity_id();
    let address = JoltAddress::new(missing_identity, "/profile").unwrap();
    let resp = client
        .post(format!("{}/api/v1/resolve", base_url(port_b)))
        .json(&serde_json::json!({ "address": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "identity_provider_not_found");
    assert!(body["error"].as_str().unwrap().contains("relay mesh"));

    handle_a.shutdown().await.ok();
    handle_b.shutdown().await.ok();
}

#[tokio::test]
async fn test_connect_peer_endpoint() {
    let p2p_a = free_tcp_port();
    let (_port_a, handle_a, _dir_a) = start_test_server_with_tcp_port(p2p_a).await;
    let (port_b, handle_b, _dir_b) = start_test_server_with_tcp_port(free_tcp_port()).await;
    let client = reqwest::Client::new();

    let peer_a = handle_a.status().await.unwrap().peer_id;
    let multiaddr = format!("/ip4/127.0.0.1/tcp/{p2p_a}/p2p/{peer_a}");

    let resp = client
        .post(format!("{}/api/v1/peers/connect", base_url(port_b)))
        .json(&serde_json::json!({ "multiaddr": multiaddr }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["peer_id"].as_str().unwrap(), peer_a);

    handle_a.shutdown().await.ok();
    handle_b.shutdown().await.ok();
}

#[tokio::test]
async fn test_two_local_tcp_daemons_publish_fetch_over_api() {
    let p2p_a = free_tcp_port();
    let p2p_b = free_tcp_port();
    let (port_a, handle_a, _dir_a) = start_test_server_with_tcp_port(p2p_a).await;
    let (port_b, handle_b, _dir_b) = start_test_server_with_tcp_port(p2p_b).await;
    let client = reqwest::Client::new();

    let peer_a = handle_a.status().await.unwrap().peer_id;
    let multiaddr = format!("/ip4/127.0.0.1/tcp/{p2p_a}/p2p/{peer_a}");

    let connect_resp = client
        .post(format!("{}/api/v1/peers/connect", base_url(port_b)))
        .json(&serde_json::json!({ "multiaddr": multiaddr }))
        .send()
        .await
        .unwrap();
    assert_eq!(connect_resp.status(), 200);

    let connected = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let status = handle_b.status().await.unwrap();
            if status.connected_peers > 0 {
                break status;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("node B did not connect to node A");
    assert!(connected.direct_peers >= 1);

    let original_data = b"local two node dashboard demo";
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(original_data.to_vec()).file_name("demo.txt"),
    );
    let pub_resp = client
        .post(format!("{}/api/v1/publish", base_url(port_a)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(pub_resp.status(), 200);
    let pub_body: serde_json::Value = pub_resp.json().await.unwrap();
    let content_id = pub_body["content_id"].as_str().unwrap().to_string();

    let fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(port_b)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(fetch_resp.status(), 200);
    let fetch_body: serde_json::Value = fetch_resp.json().await.unwrap();
    let data: Vec<u8> = fetch_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(data, original_data);

    let status_b = handle_b.status().await.unwrap();
    assert_eq!(status_b.cached_count, 1);

    handle_a.shutdown().await.ok();
    handle_b.shutdown().await.ok();
}

#[tokio::test]
async fn test_publish_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"hello from http".to_vec()).file_name("test.txt"),
    );

    let resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["content_id"].is_string());
    assert!(!body["content_id"].as_str().unwrap().is_empty());
    assert_eq!(body["size"], 15);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_publish_endpoint_can_bind_content_to_jolt_path() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let path = "/hello";

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"hello from signed path".to_vec())
                .file_name("hello.txt"),
        )
        .text("path", path.to_string());

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = ContentId::from_bytes(b"hello from signed path");
    assert_eq!(published["content_id"], content_id.to_string());
    assert_eq!(published["path"], path);
    let address = published["address"].as_str().unwrap();
    assert!(address.ends_with(".jolt/hello"));
    assert_eq!(published["latest_sequence"], 0);

    let resolve_resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({ "address": address }))
        .send()
        .await
        .unwrap();

    assert_eq!(resolve_resp.status(), 200);
    let resolved: serde_json::Value = resolve_resp.json().await.unwrap();
    assert_eq!(resolved["address"], address);
    assert_eq!(resolved["path"], path);
    assert_eq!(resolved["content_id"], content_id.to_string());
    assert_eq!(resolved["latest_sequence"], 0);
    assert_eq!(resolved["source"], "cache");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_published_inventory_endpoint_shows_local_path_publish() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let path = "/space/post";
    let data = b"inventory local path publish";

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(data.to_vec()).file_name("post.txt"),
        )
        .text("path", path.to_string());

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let content_id = ContentId::from_bytes(data).to_string();

    let inventory_resp = client
        .get(format!("{}/api/v1/published", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(inventory_resp.status(), 200);
    let inventory: serde_json::Value = inventory_resp.json().await.unwrap();
    let items = inventory.as_array().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["path"], path);
    assert_eq!(item["address"], published["address"]);
    assert_eq!(item["content_id"], content_id);
    assert_eq!(item["size"], data.len() as u64);
    assert_eq!(item["local_sequence"], 0);
    assert_eq!(item["pin_state"], "local_only");
    assert!(item["relay"].is_null());
    assert!(item["pinned_content_id"].is_null());
    assert!(item["pinned_sequence"].is_null());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_publish_endpoint_rejects_invalid_jolt_path() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"should not publish".to_vec()).file_name("bad.txt"),
        )
        .text("path", "/bad path");

    let resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("path"));
    let status = handle.status().await.unwrap();
    assert_eq!(status.published_count, 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_publish_invalid_request() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    // POST without multipart -- should get 4xx
    let resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .body("not multipart")
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_client_error());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    // Publish first
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"fetch me via http".to_vec()).file_name("test.txt"),
    );

    let pub_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    let pub_body: serde_json::Value = pub_resp.json().await.unwrap();
    let content_id = pub_body["content_id"].as_str().unwrap().to_string();

    // Fetch
    let resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["content_id"], content_id);
    assert_eq!(body["size"], 17);

    // Verify data is base64 or raw bytes encoded in JSON
    // The data field is a Vec<u8> serialized as JSON array of numbers
    let data = body["data"].as_array().unwrap();
    let bytes: Vec<u8> = data.iter().map(|v| v.as_u64().unwrap() as u8).collect();
    assert_eq!(bytes, b"fetch me via http");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_endpoint_accepts_jolt_address() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let original_data = b"fetch me through a jolt address";

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(original_data.to_vec()).file_name("address.txt"),
        )
        .text("path", "/address-test");

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    let address = published["address"].as_str().unwrap();

    let fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": address }))
        .send()
        .await
        .unwrap();

    assert_eq!(fetch_resp.status(), 200);
    let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
    assert_eq!(fetched["content_id"], published["content_id"]);
    assert_eq!(fetched["size"], original_data.len() as u64);
    let data: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(data, original_data);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_endpoint_distinguishes_unresolved_jolt_address() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let unknown_owner = NodeIdentity::generate();
    let address = JoltAddress::new(unknown_owner.identity_id(), "/missing").unwrap();

    let resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": address.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["code"], "no_bootstrap_relays");
    assert!(body["error"].as_str().unwrap().contains("jolt:update-log"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_endpoint_rejects_malformed_jolt_address() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": "alice.jolt/profile" }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["error"].as_str().unwrap().contains("identity"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_endpoint_distinguishes_resolved_but_unavailable_content() {
    let dir = tempfile::tempdir().unwrap();
    let owner = NodeIdentity::generate();
    let identity_id = owner.identity_id();
    let address = JoltAddress::new(identity_id.clone(), "/missing-content").unwrap();
    let missing_content = ContentId::from_bytes(b"content not held by any node");
    let update_log = vec![UpdateLogEntry::genesis(
        owner.public_key_bytes(),
        UpdateAction::SetPath {
            path: "/missing-content".to_string(),
            content_id: missing_content.clone(),
        },
        |bytes| owner.sign(bytes),
    )
    .unwrap()];
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(owner, store, NetworkConfig::test_config()).unwrap();
    node.store_verified_update_log(identity_id, update_log)
        .unwrap();

    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": address.to_string() }))
        .send()
        .await
        .unwrap();

    let status = resp.status().as_u16();
    assert!(
        status == 404 || status == 504,
        "Expected content fetch failure, got {status}"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["code"] == "content_provider_not_found" || body["code"] == "content_fetch_failed",
        "expected structured content failure code, got {body}"
    );
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains(&missing_content.to_string()) || error.contains("timed out"),
        "expected content fetch error, got {error}"
    );
    assert!(!error.contains("jolt:update-log"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_fetch_not_found() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "content_id": "nonexistent_id" }))
        .send()
        .await
        .unwrap();

    // Returns 404 (content not found) or 504 (timeout) since content doesn't exist
    // and there are no peers to fetch from. Both are valid error responses.
    let status = resp.status().as_u16();
    assert!(
        status == 404 || status == 504,
        "Expected 404 or 504, got {status}"
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_cache_stats_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/cache/stats", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["cached_items"], 0);
    assert!(body["max_size"].as_u64().unwrap() > 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_cache_list_endpoint() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/api/v1/cache/entries", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.is_array());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_publish_then_fetch_integration() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let original_data = b"end to end integration test data";

    // Publish
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(original_data.to_vec()).file_name("integration.txt"),
    );
    let pub_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(pub_resp.status(), 200);
    let pub_body: serde_json::Value = pub_resp.json().await.unwrap();
    let content_id = pub_body["content_id"].as_str().unwrap().to_string();

    // Fetch
    let fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "content_id": content_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(fetch_resp.status(), 200);
    let fetch_body: serde_json::Value = fetch_resp.json().await.unwrap();

    // Compare bytes
    let data: Vec<u8> = fetch_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(data, original_data);

    // Verify status reflects the published content
    let status_resp = client
        .get(format!("{}/api/v1/status", base_url(port)))
        .send()
        .await
        .unwrap();
    let status_body: serde_json::Value = status_resp.json().await.unwrap();
    assert_eq!(status_body["published_count"], 1);

    handle.shutdown().await.ok();
}
