use jolt_core::{
    generate_identity_encryption_keypair, ContentId, DeviceAuthorizationOperation,
    DeviceAuthorizationRecord, DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
    EncryptedObjectEnvelope, EncryptedObjectRecipient, IdentityEncryptionKey,
    IdentityEncryptionKeyRecord, IdentityId, JoltAddress, PinRequest, RelayRecord,
    RelayRecordCapability, UpdateAction, UpdateLogEntry, IDENTITY_AUTHORITY_PATH,
    IDENTITY_ENCRYPTION_KEYS_PATH, SIGNED_REACHABILITY_PATH,
};
use jolt_identity::NodeIdentity;
use jolt_network::{
    DaemonHandle, HomeRelayCapability, HomeRelayConfig, Multiaddr, NetworkConfig, NetworkNode,
    PeerId,
};
use jolt_server::identity_recovery::IdentityRecoveryStore;
use jolt_store::{CacheConfig, ContentStore};
use std::path::PathBuf;
use std::str::FromStr;
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
    start_test_server_with_identity_and_session_path(identity, session_path, dir).await
}

async fn start_test_server_with_identity_and_session_path(
    identity: NodeIdentity,
    session_path: PathBuf,
    dir: tempfile::TempDir,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    let local_identity_address = identity.jolt_address().to_string();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
    node.set_fetch_timeout(std::time::Duration::from_secs(2));
    node.set_resolve_timeout(std::time::Duration::from_secs(2));

    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let handle = DaemonHandle::new_with_local_identity(cmd_tx, local_identity_address);
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

async fn start_test_server_with_network_settings_path(
    settings_path: PathBuf,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let local_identity_address = identity.jolt_address().to_string();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
    node.set_fetch_timeout(std::time::Duration::from_secs(2));
    node.set_resolve_timeout(std::time::Duration::from_secs(2));

    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let handle = DaemonHandle::new_with_local_identity(cmd_tx, local_identity_address);
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });

    let sessions =
        jolt_server::session_store::AppSessionStore::open(dir.path().join("app-sessions.json"))
            .unwrap();
    let network_settings =
        jolt_server::network_settings::NetworkSettingsStore::open(settings_path).unwrap();
    let (port, _server_handle) =
        jolt_server::server::start_server_with_session_store_and_network_settings(
            handle.clone(),
            0,
            sessions,
            network_settings,
        )
        .await
        .unwrap();

    (port, handle, dir)
}

async fn start_test_server_with_profile_dir(
    profile_dir: PathBuf,
) -> (u16, DaemonHandle, tempfile::TempDir) {
    let holder = tempfile::tempdir().unwrap();
    let identity_dir = profile_dir.join("identity");
    let content_store_dir = profile_dir.join("data");
    let identity = NodeIdentity::load_or_generate(&identity_dir).unwrap();
    let store = ContentStore::open(&content_store_dir, CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
    node.set_fetch_timeout(std::time::Duration::from_secs(2));
    node.set_resolve_timeout(std::time::Duration::from_secs(2));

    let (cmd_tx, cmd_rx) = mpsc::channel(64);
    let status_handle = DaemonHandle::new(cmd_tx.clone());
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });
    let local_identity_address = status_handle.status().await.unwrap().identity_address;
    let handle = DaemonHandle::new_with_local_identity(cmd_tx, local_identity_address);

    let sessions =
        jolt_server::session_store::AppSessionStore::open(profile_dir.join("app-sessions.json"))
            .unwrap();
    let network_settings =
        jolt_server::network_settings::NetworkSettingsStore::open(profile_dir.join("network.json"))
            .unwrap();
    let identity_recovery = IdentityRecoveryStore::new(
        identity_dir,
        content_store_dir,
        Some("integration-test".to_string()),
    );
    let (port, _server_handle) = jolt_server::server::start_server_with_explicit_stores(
        handle.clone(),
        0,
        sessions,
        network_settings,
        identity_recovery,
    )
    .await
    .unwrap();

    (port, handle, holder)
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
    let status_handle = DaemonHandle::new(cmd_tx.clone());

    // Run daemon loop in background
    tokio::spawn(async move {
        node.run_daemon_loop(cmd_rx).await;
    });
    let local_identity_address = status_handle.status().await.unwrap().identity_address;
    let handle = DaemonHandle::new_with_local_identity(cmd_tx, local_identity_address);

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

fn device_authority_record(
    root: &NodeIdentity,
    device: &NodeIdentity,
    device_id: &str,
) -> DeviceAuthorizationRecord {
    DeviceAuthorizationRecord::genesis(
        root.public_key_bytes(),
        root.identity_id(),
        DeviceAuthorizationOperation::authorize_device(
            device_id,
            device.public_key_bytes(),
            vec!["identity:write".to_string()],
            Some("Laptop".to_string()),
            1_780_579_200,
        ),
        1_780_579_200,
        |bytes| root.sign(bytes),
    )
    .unwrap()
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

async fn encrypted_spoke_reply_for_local_identity(handle: &DaemonHandle) -> (String, Vec<u8>) {
    let bob_status = handle.status().await.unwrap();
    let bob_identity = bob_status
        .identity_address
        .trim_end_matches('/')
        .to_string();
    let bob_identity_id = IdentityId::from_str(bob_identity.trim_end_matches(".jolt")).unwrap();
    let bob_key = handle.ensure_local_identity_encryption_key().await.unwrap();
    let alice = NodeIdentity::generate();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let envelope = EncryptedObjectEnvelope::encrypt(
        alice.public_key_bytes(),
        alice.identity_id(),
        br#"{"post":"bob/post/1","body":"hello"}"#,
        "application/json".to_string(),
        Some("application/vnd.spoke.reply+json".to_string()),
        vec![EncryptedObjectRecipient {
            identity: bob_identity_id,
            key: bob_key,
        }],
        now,
        |bytes| alice.sign(bytes),
    )
    .unwrap();
    (bob_identity, envelope.to_bytes().unwrap())
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

fn relay_config_for(owner: &NodeIdentity) -> NetworkConfig {
    let mut config = relay_config();
    config
        .relay_pin_policy
        .allowed_identities
        .insert(owner.identity_id().to_string());
    config
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
    assert!(body.contains("Jolt Console"));
    assert!(body.contains("first-party desktop control surface"));
    assert!(!body.contains("Jolt Node Console"));
    assert!(!body.contains("Jolt Debug Dashboard"));
    assert!(!body.contains("/debug/dashboard"));
    assert!(!body.contains("/api/v1/publish"));
    assert!(!body.contains("publish-path"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_old_dashboard_paths_are_retired() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    for path in ["/dashboard", "/debug/dashboard"] {
        let resp = client
            .get(format!("{}{}", base_url(port), path))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 404, "{path} should be retired");
    }

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
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "app_origin": "http://127.0.0.1:5174",
            "requested_identity": local_identity,
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
    assert_eq!(requests[0]["requested_identity"], local_identity);
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
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": local_identity,
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
            "identity": local_identity,
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
    assert_eq!(approved["identity"], local_identity);

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
    assert_eq!(sessions[0]["identity"], local_identity);
    assert!(sessions[0].get("session_token").is_none());
    assert!(sessions[0].get("token_hash").is_none());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_approve_app_session_request_for_local_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": null,
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

    let pending_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    assert_eq!(
        pending.as_array().unwrap()[0]["requested_identity"],
        local_identity
    );

    let approve_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/approve",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "identity": null,
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
    assert_eq!(approved["status"], "active");
    assert_eq!(approved["identity"], local_identity);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_selected_local_identity_is_used_for_app_session_approval() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.local_identity_address().unwrap().to_string();

    let identities_resp = client
        .get(format!("{}/admin/v1/identities", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(identities_resp.status(), 200);
    let identities: serde_json::Value = identities_resp.json().await.unwrap();
    assert_eq!(identities["active_identity"], daemon_identity);
    assert_eq!(identities["identities"].as_array().unwrap().len(), 1);
    assert_eq!(identities["identities"][0]["address"], daemon_identity);

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();
    assert_ne!(work_identity, daemon_identity);
    assert_eq!(created["label"], "Work");
    assert_eq!(created["active"], false);

    let select_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_resp.status(), 200);
    let selected: serde_json::Value = select_resp.json().await.unwrap();
    assert_eq!(selected["active_identity"], work_identity);
    assert_eq!(selected["identities"].as_array().unwrap().len(), 2);

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": null,
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
            "identity": null,
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(approved["identity"], work_identity);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_app_grants_are_scoped_to_selected_local_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.local_identity_address().unwrap().to_string();

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();

    let daemon_token = approve_app_session(
        &client,
        port,
        &daemon_identity,
        &["resolve:public", "fetch:public"],
    )
    .await;
    assert!(!daemon_token.is_empty());

    let select_work_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_work_resp.status(), 200);

    let work_token = approve_app_session(
        &client,
        port,
        &work_identity,
        &["resolve:public", "fetch:public"],
    )
    .await;
    assert!(!work_token.is_empty());

    let work_sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(work_sessions_resp.status(), 200);
    let work_sessions: serde_json::Value = work_sessions_resp.json().await.unwrap();
    let work_sessions = work_sessions.as_array().unwrap();
    assert_eq!(work_sessions.len(), 1);
    assert_eq!(work_sessions[0]["identity"], work_identity);
    let work_session_id = work_sessions[0]["session_id"].as_str().unwrap().to_string();

    let select_daemon_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": daemon_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_daemon_resp.status(), 200);

    let daemon_sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(daemon_sessions_resp.status(), 200);
    let daemon_sessions: serde_json::Value = daemon_sessions_resp.json().await.unwrap();
    let daemon_sessions = daemon_sessions.as_array().unwrap();
    assert_eq!(daemon_sessions.len(), 1);
    assert_eq!(daemon_sessions[0]["identity"], daemon_identity);
    let daemon_session_id = daemon_sessions[0]["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let cross_identity_revoke = client
        .post(format!(
            "{}/admin/v1/app-sessions/{work_session_id}/revoke",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(cross_identity_revoke.status(), 404);

    let revoke_daemon_resp = client
        .post(format!(
            "{}/admin/v1/app-sessions/{daemon_session_id}/revoke",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_daemon_resp.status(), 200);
    let revoked_daemon: serde_json::Value = revoke_daemon_resp.json().await.unwrap();
    assert_eq!(revoked_daemon["identity"], daemon_identity);
    assert_eq!(revoked_daemon["status"], "revoked");

    let select_work_again_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_work_again_resp.status(), 200);

    let work_sessions_resp = client
        .get(format!("{}/admin/v1/app-sessions", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(work_sessions_resp.status(), 200);
    let work_sessions: serde_json::Value = work_sessions_resp.json().await.unwrap();
    let work_sessions = work_sessions.as_array().unwrap();
    assert_eq!(work_sessions.len(), 1);
    assert_eq!(work_sessions[0]["session_id"], work_session_id);
    assert_eq!(work_sessions[0]["identity"], work_identity);
    assert_eq!(work_sessions[0]["status"], "active");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_app_requests_are_scoped_to_selected_local_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.local_identity_address().unwrap().to_string();

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();

    let select_work_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_work_resp.status(), 200);

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": null,
            "requested_capabilities": ["resolve:public"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(request_resp.status(), 200);
    let requested: serde_json::Value = request_resp.json().await.unwrap();
    let request_id = requested["request_id"].as_str().unwrap();

    let work_requests_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(work_requests_resp.status(), 200);
    let work_requests: serde_json::Value = work_requests_resp.json().await.unwrap();
    let work_requests = work_requests.as_array().unwrap();
    assert_eq!(work_requests.len(), 1);
    assert_eq!(work_requests[0]["request_id"], request_id);
    assert_eq!(work_requests[0]["requested_identity"], work_identity);
    assert_eq!(work_requests[0]["status"], "pending");

    let select_daemon_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": daemon_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_daemon_resp.status(), 200);

    let daemon_requests_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(daemon_requests_resp.status(), 200);
    let daemon_requests: serde_json::Value = daemon_requests_resp.json().await.unwrap();
    assert_eq!(daemon_requests.as_array().unwrap().len(), 0);

    let select_work_again_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_work_again_resp.status(), 200);

    let reject_resp = client
        .post(format!(
            "{}/admin/v1/app-requests/{request_id}/reject",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(reject_resp.status(), 200);

    let rejected_requests_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected_requests_resp.status(), 200);
    let rejected_requests: serde_json::Value = rejected_requests_resp.json().await.unwrap();
    let rejected_requests = rejected_requests.as_array().unwrap();
    assert_eq!(rejected_requests.len(), 1);
    assert_eq!(rejected_requests[0]["request_id"], request_id);
    assert_eq!(rejected_requests[0]["requested_identity"], work_identity);
    assert_eq!(rejected_requests[0]["status"], "rejected");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_delete_generated_local_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.local_identity_address().unwrap().to_string();

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();

    let select_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_resp.status(), 200);

    let delete_resp = client
        .delete(format!(
            "{}/admin/v1/identities/{}",
            base_url(port),
            work_identity
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_resp.status(), 200);
    let deleted: serde_json::Value = delete_resp.json().await.unwrap();
    assert_eq!(deleted["active_identity"], daemon_identity);
    assert_eq!(deleted["identities"].as_array().unwrap().len(), 1);
    assert_eq!(deleted["identities"][0]["address"], daemon_identity);
    assert_eq!(deleted["identities"][0]["active"], true);

    let delete_daemon_resp = client
        .delete(format!(
            "{}/admin/v1/identities/{}",
            base_url(port),
            daemon_identity
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(delete_daemon_resp.status(), 400);
    let body: serde_json::Value = delete_daemon_resp.json().await.unwrap();
    assert_eq!(body["code"], "local_identity_protected");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_exports_selected_generated_local_identity_recovery_bundle() {
    let profile = tempfile::tempdir().unwrap();
    let (port, handle, _dir) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.local_identity_address().unwrap().to_string();

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();
    assert_ne!(work_identity, daemon_identity);

    let export_resp = client
        .post(format!("{}/admin/v1/identities/export", base_url(port)))
        .json(&serde_json::json!({
            "identity": work_identity,
            "passphrase": "",
            "label": "Work laptop"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(export_resp.status(), 200);
    let exported: serde_json::Value = export_resp.json().await.unwrap();
    assert_eq!(exported["identity"], work_identity);
    assert_eq!(exported["bundle"]["identity"], work_identity);
    assert_eq!(exported["encryption_key_count"], 0);

    let daemon_export_resp = client
        .post(format!("{}/admin/v1/identities/export", base_url(port)))
        .json(&serde_json::json!({
            "identity": daemon_identity,
            "passphrase": "",
            "label": "Default laptop"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(daemon_export_resp.status(), 200);
    let daemon_exported: serde_json::Value = daemon_export_resp.json().await.unwrap();
    assert_eq!(daemon_exported["identity"], daemon_identity);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_imports_identity_recovery_bundle_as_local_identity_without_replacing_daemon() {
    let source_profile = tempfile::tempdir().unwrap();
    let target_profile = tempfile::tempdir().unwrap();
    let (source_port, source_handle, _source_holder) =
        start_test_server_with_profile_dir(source_profile.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let source_identity = source_handle.local_identity_address().unwrap().to_string();
    source_handle
        .ensure_local_identity_encryption_key()
        .await
        .unwrap();

    let export_resp = client
        .post(format!(
            "{}/admin/v1/identities/export",
            base_url(source_port)
        ))
        .json(&serde_json::json!({
            "passphrase": "",
            "label": "Recovered"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(export_resp.status(), 200);
    let exported: serde_json::Value = export_resp.json().await.unwrap();
    let bundle = exported["bundle"].clone();

    let (target_port, target_handle, _target_holder) =
        start_test_server_with_profile_dir(target_profile.path().to_path_buf()).await;
    let target_identity = target_handle.local_identity_address().unwrap().to_string();
    assert_ne!(target_identity, source_identity);

    let import_resp = client
        .post(format!(
            "{}/admin/v1/identities/import",
            base_url(target_port)
        ))
        .json(&serde_json::json!({
            "passphrase": "",
            "bundle": bundle,
            "as_local_identity": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(import_resp.status(), 200);
    let imported: serde_json::Value = import_resp.json().await.unwrap();
    assert_eq!(imported["identity"], source_identity);
    assert_eq!(imported["imported"], true);
    assert_eq!(imported["restart_required"], false);
    assert_eq!(imported["app_sessions_imported"], false);

    assert_eq!(
        target_handle.status().await.unwrap().identity_address,
        target_identity
    );

    let identities_resp = client
        .get(format!("{}/admin/v1/identities", base_url(target_port)))
        .send()
        .await
        .unwrap();
    assert_eq!(identities_resp.status(), 200);
    let identities: serde_json::Value = identities_resp.json().await.unwrap();
    assert_eq!(identities["active_identity"], target_identity);
    let identities = identities["identities"].as_array().unwrap();
    assert_eq!(identities.len(), 2);
    assert!(identities
        .iter()
        .any(|identity| identity["address"] == target_identity));
    assert!(identities.iter().any(|identity| {
        identity["address"] == source_identity && identity["label"] == "Recovered"
    }));

    target_handle.shutdown().await.ok();

    let (restarted_port, restarted_handle, _restarted_holder) =
        start_test_server_with_profile_dir(target_profile.path().to_path_buf()).await;
    let identities_resp = client
        .get(format!("{}/admin/v1/identities", base_url(restarted_port)))
        .send()
        .await
        .unwrap();
    assert_eq!(identities_resp.status(), 200);
    let identities: serde_json::Value = identities_resp.json().await.unwrap();
    assert!(identities["identities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|identity| identity["address"] == source_identity));

    let reexport_resp = client
        .post(format!(
            "{}/admin/v1/identities/export",
            base_url(restarted_port)
        ))
        .json(&serde_json::json!({
            "identity": source_identity,
            "passphrase": "recovered-again"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reexport_resp.status(), 200);
    let reexported: serde_json::Value = reexport_resp.json().await.unwrap();
    assert_eq!(reexported["encryption_key_count"], 1);

    source_handle.shutdown().await.ok();
    restarted_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_export_and_import_identity_recovery_bundle() {
    let source_root = tempfile::tempdir().unwrap();
    let target_root = tempfile::tempdir().unwrap();
    let source_profile = source_root.path().join("source-profile");
    let target_profile = target_root.path().join("target-profile");

    let (source_port, source_handle, _source_holder) =
        start_test_server_with_profile_dir(source_profile.clone()).await;
    let client = reqwest::Client::new();
    let source_identity = source_handle.status().await.unwrap().identity_address;
    source_handle
        .ensure_local_identity_encryption_key()
        .await
        .unwrap();

    let source_token = approve_app_session(
        &client,
        source_port,
        &source_identity,
        &["resolve:public", "fetch:public"],
    )
    .await;
    assert!(!source_token.is_empty());

    let export_resp = client
        .post(format!(
            "{}/admin/v1/identities/export",
            base_url(source_port)
        ))
        .json(&serde_json::json!({
            "passphrase": "correct horse battery staple",
            "label": "integration export"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(export_resp.status(), 200);
    let exported: serde_json::Value = export_resp.json().await.unwrap();
    assert_eq!(exported["identity"], source_identity);
    assert_eq!(exported["encryption_key_count"], 1);
    let bundle = exported["bundle"].clone();

    let app_export_attempt = client
        .post(format!(
            "{}/app/v1/identities/export",
            base_url(source_port)
        ))
        .bearer_auth(&source_token)
        .json(&serde_json::json!({ "passphrase": "correct horse battery staple" }))
        .send()
        .await
        .unwrap();
    assert_eq!(app_export_attempt.status(), 404);

    let (target_port, target_handle, _target_holder) =
        start_test_server_with_profile_dir(target_profile.clone()).await;
    let original_target_identity = target_handle.status().await.unwrap().identity_address;
    assert_ne!(original_target_identity, source_identity);

    let refused = client
        .post(format!(
            "{}/admin/v1/identities/import",
            base_url(target_port)
        ))
        .json(&serde_json::json!({
            "passphrase": "correct horse battery staple",
            "bundle": bundle
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 409);

    let import_resp = client
        .post(format!(
            "{}/admin/v1/identities/import",
            base_url(target_port)
        ))
        .json(&serde_json::json!({
            "passphrase": "correct horse battery staple",
            "bundle": bundle,
            "allow_overwrite": true
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(import_resp.status(), 200);
    let imported: serde_json::Value = import_resp.json().await.unwrap();
    assert_eq!(imported["identity"], source_identity);
    assert_eq!(imported["imported"], true);
    assert_eq!(imported["restart_required"], true);
    assert_eq!(imported["encryption_key_count"], 1);
    assert_eq!(imported["app_sessions_imported"], false);

    let target_sessions_before_restart = client
        .get(format!("{}/admin/v1/app-sessions", base_url(target_port)))
        .send()
        .await
        .unwrap();
    assert_eq!(target_sessions_before_restart.status(), 200);
    let target_sessions: serde_json::Value = target_sessions_before_restart.json().await.unwrap();
    assert_eq!(target_sessions.as_array().unwrap().len(), 0);

    target_handle.shutdown().await.ok();

    let (restarted_port, restarted_handle, _restarted_holder) =
        start_test_server_with_profile_dir(target_profile).await;
    let restarted_identity = restarted_handle.status().await.unwrap().identity_address;
    assert_eq!(restarted_identity, source_identity);

    let sessions_after_restart = client
        .get(format!(
            "{}/admin/v1/app-sessions",
            base_url(restarted_port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(sessions_after_restart.status(), 200);
    let sessions: serde_json::Value = sessions_after_restart.json().await.unwrap();
    assert_eq!(sessions.as_array().unwrap().len(), 0);

    let imported_token = approve_app_session(
        &client,
        restarted_port,
        &source_identity,
        &["publish:/app/*", "enumerate:self:/app/*", "fetch:public"],
    )
    .await;
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(br#"{"title":"portable identity"}"#.to_vec())
                .file_name("record.json"),
        )
        .text("path", "/app/items/imported".to_string());
    let appended = client
        .post(format!("{}/app/v1/append", base_url(restarted_port)))
        .bearer_auth(&imported_token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(appended.status(), 200);
    let appended: serde_json::Value = appended.json().await.unwrap();
    assert_eq!(
        appended["address"],
        format!("{source_identity}/app/items/imported")
    );

    let enumerated = client
        .post(format!("{}/app/v1/enumerate", base_url(restarted_port)))
        .bearer_auth(&imported_token)
        .json(&serde_json::json!({
            "identity": source_identity,
            "path_prefix": "/app/items/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(enumerated.status(), 200);
    let records: serde_json::Value = enumerated.json().await.unwrap();
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["path"], "/app/items/imported");

    source_handle.shutdown().await.ok();
    restarted_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_device_authority_can_authorize_and_revoke_local_device() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.status().await.unwrap().identity_address;

    let initial_resp = client
        .get(format!("{}/admin/v1/device-authority", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(initial_resp.status(), 200);
    let initial: serde_json::Value = initial_resp.json().await.unwrap();
    assert_eq!(initial["identity"], daemon_identity);
    assert_eq!(initial["latest_sequence"], 0);
    assert_eq!(initial["devices"].as_array().unwrap().len(), 1);
    assert_eq!(initial["devices"][0]["device_id"], "dev_legacy_root");
    assert_eq!(initial["devices"][0]["status"], "active");

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Laptop" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);
    let authorized: serde_json::Value = authorize_resp.json().await.unwrap();
    assert_eq!(authorized["identity"], daemon_identity);
    assert_eq!(authorized["latest_sequence"], 1);
    let generated_device_id = authorized["device"]["device_id"].as_str().unwrap();
    assert_ne!(generated_device_id, "dev_legacy_root");
    assert_eq!(authorized["device"]["label"], "Laptop");
    assert_eq!(authorized["device"]["status"], "active");
    assert_eq!(
        authorized["device"]["encryption_keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/{generated_device_id}/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 3,
            "reason": "lost_device"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);
    let revoked: serde_json::Value = revoke_resp.json().await.unwrap();
    assert_eq!(revoked["latest_sequence"], 2);
    assert_eq!(revoked["device"]["device_id"], generated_device_id);
    assert_eq!(revoked["device"]["status"], "revoked");
    assert_eq!(revoked["device"]["accepted_through_device_sequence"], 3);

    let resolve_resp = client
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({
            "address": format!("{daemon_identity}{IDENTITY_AUTHORITY_PATH}")
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resolve_resp.status(), 200);
    let resolved: serde_json::Value = resolve_resp.json().await.unwrap();
    assert_eq!(resolved["path"], IDENTITY_AUTHORITY_PATH);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_encrypted_publish_excludes_revoked_authorized_devices() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;

    let revoked_device_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Lost phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(revoked_device_resp.status(), 200);
    let revoked_device: serde_json::Value = revoked_device_resp.json().await.unwrap();
    let revoked_device_id = revoked_device["device"]["device_id"].as_str().unwrap();

    let active_device_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Replacement phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(active_device_resp.status(), 200);

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/{revoked_device_id}/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 0,
            "reason": "lost_device"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);

    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/revoked-excluded",
            "plaintext": b"not for revoked devices".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["recipient_count"], 2);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_device_authority_list_is_idempotent() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let daemon_identity = handle.status().await.unwrap().identity_address;

    let first_resp = client
        .get(format!("{}/admin/v1/device-authority", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(first_resp.status(), 200);
    let first: serde_json::Value = first_resp.json().await.unwrap();
    assert_eq!(first["identity"], daemon_identity);
    assert_eq!(first["latest_sequence"], 0);
    assert_eq!(first["devices"].as_array().unwrap().len(), 1);

    let second_resp = client
        .get(format!("{}/admin/v1/device-authority", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(second_resp.status(), 200);
    let second: serde_json::Value = second_resp.json().await.unwrap();
    assert_eq!(second["identity"], first["identity"]);
    assert_eq!(second["latest_sequence"], first["latest_sequence"]);
    assert_eq!(second["devices"], first["devices"]);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_device_authority_rejects_unknown_device_revocation() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/dev_missing/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 3,
            "reason": "not_authorized"
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(revoke_resp.status(), 400);
    let body: serde_json::Value = revoke_resp.json().await.unwrap();
    assert_eq!(body["code"], "device_authority_invalid");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("cannot revoke unknown device dev_missing"));

    let authority_resp = client
        .get(format!("{}/admin/v1/device-authority", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(authority_resp.status(), 200);
    let authority: serde_json::Value = authority_resp.json().await.unwrap();
    assert_eq!(authority["latest_sequence"], 0);
    assert_eq!(authority["devices"].as_array().unwrap().len(), 1);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_device_authority_can_continue_after_rejected_revocation() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let rejected_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/dev_missing/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 3,
            "reason": "not_authorized"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rejected_resp.status(), 400);

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Recovered laptop" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);
    let authorized: serde_json::Value = authorize_resp.json().await.unwrap();
    assert_eq!(authorized["latest_sequence"], 1);
    assert_eq!(authorized["devices"].as_array().unwrap().len(), 2);
    let generated_device_id = authorized["device"]["device_id"].as_str().unwrap();

    let revoke_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/{generated_device_id}/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 4,
            "reason": "rotated"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), 200);
    let revoked: serde_json::Value = revoke_resp.json().await.unwrap();
    assert_eq!(revoked["latest_sequence"], 2);
    assert_eq!(revoked["device"]["status"], "revoked");
    assert_eq!(revoked["device"]["accepted_through_device_sequence"], 4);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_cannot_grant_publish_capability_to_non_daemon_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();

    let create_resp = client
        .post(format!("{}/admin/v1/identities", base_url(port)))
        .json(&serde_json::json!({ "label": "Work" }))
        .send()
        .await
        .unwrap();
    assert_eq!(create_resp.status(), 200);
    let created: serde_json::Value = create_resp.json().await.unwrap();
    let work_identity = created["address"].as_str().unwrap().to_string();

    let select_resp = client
        .post(format!("{}/admin/v1/identities/active", base_url(port)))
        .json(&serde_json::json!({ "identity": work_identity }))
        .send()
        .await
        .unwrap();
    assert_eq!(select_resp.status(), 200);

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": null,
            "requested_capabilities": ["publish:/pastes/*"]
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
            "identity": null,
            "capabilities": [],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 400);
    let body: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_identity_not_signable");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_cannot_grant_publish_capability_to_unknown_identity() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": "wrongidentity.jolt",
            "requested_capabilities": ["publish:/pastes/*"]
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
            "identity": "wrongidentity.jolt",
            "capabilities": ["publish:/pastes/*"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(approve_resp.status(), 400);
    let body: serde_json::Value = approve_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_identity_not_signable");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_approval_of_private_app_capabilities_publishes_identity_encryption_keys() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "app_origin": "http://127.0.0.1:5174",
            "requested_identity": identity,
            "requested_capabilities": [
                "resolve:public",
                "fetch:public",
                "encrypt:/pastes/*",
                "decrypt:/pastes/*",
                "publish:encrypted:/pastes/*"
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
            "identity": identity,
            "capabilities": [
                "resolve:public",
                "fetch:public",
                "encrypt:/pastes/*",
                "decrypt:/pastes/*",
                "publish:encrypted:/pastes/*"
            ]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);

    let status = handle.status().await.unwrap();
    assert_eq!(status.published_count, 1);

    let identity_id = JoltAddress::from_str(&identity)
        .unwrap()
        .identity()
        .to_string();
    let keys_resp = client
        .get(format!(
            "{}/api/v1/identities/{identity_id}/encryption-keys",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(keys_resp.status(), 200);
    let keys: serde_json::Value = keys_resp.json().await.unwrap();
    assert_eq!(keys["identity"], identity_id);
    assert_eq!(keys["keys"].as_array().unwrap().len(), 1);

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
    let identity = handle.status().await.unwrap().identity_address;

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": identity,
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
            "identity": identity,
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
    let identity = handle.status().await.unwrap().identity_address;

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": identity,
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
            "identity": identity,
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
    let identity = handle.status().await.unwrap().identity_address;

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": identity,
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
            "identity": identity,
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
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
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

    let requests_resp = client
        .get(format!("{}/admin/v1/app-requests", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(requests_resp.status(), 200);
    let requests: serde_json::Value = requests_resp.json().await.unwrap();
    assert_eq!(requests.as_array().unwrap().len(), 1);
    assert_eq!(requests[0]["request_id"], request_id);
    assert_eq!(requests[0]["status"], "rejected");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_revoke_active_app_session() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": local_identity,
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
            "identity": local_identity,
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
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": local_identity,
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
            "identity": local_identity,
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
async fn test_revoking_local_device_revokes_its_app_sessions() {
    let session_dir = tempfile::tempdir().unwrap();
    let session_path = session_dir.path().join("app-sessions.json");
    let (port, handle, _dir) = start_test_server_with_session_path(session_path).await;
    let client = reqwest::Client::new();
    let local_identity = handle.local_identity_address().unwrap().to_string();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": local_identity,
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
            "identity": local_identity,
            "capabilities": ["resolve:public"],
            "expires_at": null
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(approve_resp.status(), 200);
    let approved: serde_json::Value = approve_resp.json().await.unwrap();
    let session_token = approved["session_token"].as_str().unwrap();
    assert_eq!(approved["device_id"], "dev_legacy_root");

    let current_resp = client
        .get(format!("{}/app/v1/session", base_url(port)))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap();
    assert_eq!(current_resp.status(), 200);

    let revoke_device_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices/dev_legacy_root/revoke",
            base_url(port)
        ))
        .json(&serde_json::json!({
            "accepted_through_device_sequence": 0,
            "reason": "test_device_revocation"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(revoke_device_resp.status(), 200);

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
async fn test_app_can_append_and_enumerate_records_by_prefix() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &["publish:/app/*", "enumerate:self:/app/*"],
    )
    .await;

    for (path, body) in [
        ("/app/items/1", b"record one".as_slice()),
        ("/app/items/2", b"record two".as_slice()),
        ("/app/other/x", b"unrelated".as_slice()),
    ] {
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(body.to_vec()).file_name("record.json"),
            )
            .text("path", path.to_string());
        let appended = client
            .post(format!("{}/app/v1/append", base_url(port)))
            .bearer_auth(&token)
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(appended.status(), 200);
        let value: serde_json::Value = appended.json().await.unwrap();
        assert_eq!(value["path"], path);
    }

    let enumerated = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "identity": identity,
            "path_prefix": "/app/items/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(enumerated.status(), 200);
    let records: serde_json::Value = enumerated.json().await.unwrap();
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["path"], "/app/items/1");
    assert_eq!(records[1]["path"], "/app/items/2");
    assert!(!records[0]["content_id"].as_str().unwrap().is_empty());

    let outside_path = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "identity": identity,
            "path_prefix": "/spoke/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(outside_path.status(), 403);

    // Generic public resolution does not imply append-record enumeration.
    let no_read_token = approve_app_session(&client, port, &identity, &["resolve:public"]).await;
    let denied = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&no_read_token)
        .json(&serde_json::json!({
            "identity": identity,
            "path_prefix": "/app/items/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_reads_authoritative_local_record_state_across_restart() {
    let profile = tempfile::tempdir().unwrap();
    let (first_port, first_handle, _first_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let identity = first_handle.status().await.unwrap().identity_address;
    let capabilities = [
        "resolve:public",
        "fetch:public",
        "publish:/chirp/*",
        "inventory:/chirp/*",
    ];
    for incomplete_capabilities in [&["resolve:public"][..], &["fetch:public"][..]] {
        let incomplete_token =
            approve_app_session(&client, first_port, &identity, incomplete_capabilities).await;
        let denied = client
            .post(format!("{}/app/v1/records/read", base_url(first_port)))
            .bearer_auth(incomplete_token)
            .json(&serde_json::json!({ "path": "/chirp/posts/jlt_record" }))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403);
    }
    let token = approve_app_session(&client, first_port, &identity, &capabilities).await;
    let path = "/chirp/posts/jlt_record";

    let missing = client
        .post(format!("{}/app/v1/records/read", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 200);
    assert_eq!(
        missing.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({ "state": "missing", "path": path })
    );

    let stored = br#"{"version":1,"value":{"text":"Hello!"}}"#;
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(stored.to_vec()).file_name("record.json"),
        )
        .text("path", path.to_string());
    let published = client
        .post(format!("{}/app/v1/publish", base_url(first_port)))
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 200);
    let published: serde_json::Value = published.json().await.unwrap();

    let first_read = client
        .post(format!("{}/app/v1/records/read", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(first_read.status(), 200);
    let first_read: serde_json::Value = first_read.json().await.unwrap();
    assert_eq!(first_read["state"], "present");
    assert_eq!(first_read["path"], path);
    assert_eq!(first_read["content_id"], published["content_id"]);
    assert_eq!(first_read["revision"], published["revision"]);
    assert_eq!(first_read["revision"].as_str().unwrap().len(), 64);
    assert_eq!(
        first_read["data"],
        serde_json::Value::Array(
            stored
                .iter()
                .copied()
                .map(serde_json::Value::from)
                .collect()
        )
    );

    let content_id = published["content_id"].as_str().unwrap();
    let content_path = profile
        .path()
        .join("data")
        .join("published")
        .join(content_id)
        .join("content");
    std::fs::remove_file(&content_path).unwrap();
    let unavailable = client
        .post(format!("{}/app/v1/records/read", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert!(unavailable.status() == 404 || unavailable.status() == 504);
    let unavailable: serde_json::Value = unavailable.json().await.unwrap();
    assert!(
        unavailable["code"] == "content_provider_not_found"
            || unavailable["code"] == "content_fetch_failed",
        "expected structured content failure code, got {unavailable}"
    );
    std::fs::write(content_path, stored).unwrap();

    let updated = br#"{"version":1,"value":{"text":"Edited"}}"#;
    let update_request = serde_json::json!({
        "path": path,
        "revision": first_read["revision"],
        "mutation_id": "mut_record_update",
        "data": updated.to_vec(),
    });
    let no_publish_token = approve_app_session(
        &client,
        first_port,
        &identity,
        &["resolve:public", "fetch:public"],
    )
    .await;
    let denied_update = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(no_publish_token)
        .json(&update_request)
        .send()
        .await
        .unwrap();
    assert_eq!(denied_update.status(), 403);

    let successful_update = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&update_request)
        .send()
        .await
        .unwrap();
    assert_eq!(successful_update.status(), 200);
    let successful_update: serde_json::Value = successful_update.json().await.unwrap();
    assert_eq!(successful_update["path"], path);
    assert_ne!(successful_update["revision"], first_read["revision"]);
    assert_eq!(
        successful_update["data"],
        serde_json::Value::Array(
            updated
                .iter()
                .copied()
                .map(serde_json::Value::from)
                .collect()
        )
    );

    let inventory = client
        .get(format!("{}/app/v1/published", base_url(first_port)))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(inventory.status(), 200);
    assert!(inventory
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
        .iter()
        .any(|item| {
            item["path"] == path && item["content_id"] == successful_update["content_id"]
        }));

    let retried_update = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&update_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retried_update.status(), 200);
    assert_eq!(
        retried_update.json::<serde_json::Value>().await.unwrap(),
        successful_update
    );

    let reused_mutation_id = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": first_read["revision"],
            "mutation_id": "mut_record_update",
            "data": br#"{"version":1,"value":{"text":"Different request"}}"#.to_vec(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reused_mutation_id.status(), 400);
    assert_eq!(
        reused_mutation_id
            .json::<serde_json::Value>()
            .await
            .unwrap()["code"],
        "invalid_input"
    );

    let stale_update = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": first_read["revision"],
            "mutation_id": "mut_stale_update",
            "data": br#"{"version":1,"value":{"text":"Stale"}}"#.to_vec(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale_update.status(), 409);
    assert_eq!(
        stale_update.json::<serde_json::Value>().await.unwrap()["code"],
        "record_conflict"
    );

    let concurrent_a = serde_json::json!({
        "path": path,
        "revision": successful_update["revision"],
        "mutation_id": "mut_concurrent_a",
        "data": br#"{"version":1,"value":{"text":"Concurrent A"}}"#.to_vec(),
    });
    let concurrent_b = serde_json::json!({
        "path": path,
        "revision": successful_update["revision"],
        "mutation_id": "mut_concurrent_b",
        "data": br#"{"version":1,"value":{"text":"Concurrent B"}}"#.to_vec(),
    });
    let send_a = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&concurrent_a)
        .send();
    let send_b = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&concurrent_b)
        .send();
    let (response_a, response_b) = tokio::join!(send_a, send_b);
    let response_a = response_a.unwrap();
    let response_b = response_b.unwrap();
    let statuses = [response_a.status(), response_b.status()];
    assert_eq!(
        statuses
            .iter()
            .filter(|status| status.as_u16() == 200)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| status.as_u16() == 409)
            .count(),
        1
    );
    let (winner_response, conflict_response) = if response_a.status() == 200 {
        (response_a, response_b)
    } else {
        (response_b, response_a)
    };
    let concurrent_winner: serde_json::Value = winner_response.json().await.unwrap();
    assert_eq!(
        conflict_response.json::<serde_json::Value>().await.unwrap()["code"],
        "record_conflict"
    );

    first_handle.shutdown().await.ok();

    let (second_port, second_handle, _second_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let retry_after_restart = client
        .post(format!("{}/app/v1/records/update", base_url(second_port)))
        .bearer_auth(&token)
        .json(&update_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retry_after_restart.status(), 200);
    assert_eq!(
        retry_after_restart
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        successful_update
    );

    let after_restart = client
        .post(format!("{}/app/v1/records/read", base_url(second_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(after_restart.status(), 200);
    let after_restart: serde_json::Value = after_restart.json().await.unwrap();
    assert_eq!(after_restart["state"], "present");
    assert_eq!(after_restart["path"], path);
    assert_eq!(after_restart["content_id"], concurrent_winner["content_id"]);
    assert_eq!(after_restart["revision"], concurrent_winner["revision"]);
    assert_eq!(after_restart["data"], concurrent_winner["data"]);

    second_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_delete_record_is_capability_scoped_idempotent_and_durable() {
    let profile = tempfile::tempdir().unwrap();
    let (first_port, first_handle, _first_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let identity = first_handle.status().await.unwrap().identity_address;
    let path = "/chirp/posts/jlt_deleted_by_app";
    let token = approve_app_session(
        &client,
        first_port,
        &identity,
        &[
            "publish:/chirp/*",
            "delete:/chirp/*",
            "inventory:/chirp/*",
            "resolve:public",
            "fetch:public",
        ],
    )
    .await;

    let stored = br#"{"version":1,"value":{"text":"Keep immutable bytes"}}"#;
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(stored.to_vec()).file_name("record.json"),
        )
        .text("path", path.to_string());
    let published = client
        .post(format!("{}/app/v1/publish", base_url(first_port)))
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 200);
    let published: serde_json::Value = published.json().await.unwrap();
    let delete_request = serde_json::json!({
        "path": path,
        "revision": published["revision"],
        "mutation_id": "mut_record_delete",
    });

    let no_delete_token =
        approve_app_session(&client, first_port, &identity, &["publish:/chirp/*"]).await;
    let denied = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(no_delete_token)
        .json(&delete_request)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let deleted = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(&token)
        .json(&delete_request)
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let deleted: serde_json::Value = deleted.json().await.unwrap();
    assert_eq!(deleted["path"], path);
    assert_ne!(deleted["revision"], published["revision"]);
    assert_eq!(deleted["revision"].as_str().unwrap().len(), 64);

    let read_deleted = client
        .post(format!("{}/app/v1/records/read", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(read_deleted.status(), 200);
    assert_eq!(
        read_deleted.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({
            "state": "deleted",
            "path": path,
            "revision": deleted["revision"],
        })
    );

    let inventory = client
        .get(format!("{}/app/v1/published", base_url(first_port)))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(inventory.status(), 200);
    assert!(inventory
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
        .iter()
        .all(|item| item["path"] != path));

    let old_content = client
        .post(format!("{}/app/v1/fetch", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["content_id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(old_content.status(), 200);
    assert_eq!(
        old_content.json::<serde_json::Value>().await.unwrap()["data"],
        serde_json::Value::Array(
            stored
                .iter()
                .copied()
                .map(serde_json::Value::from)
                .collect()
        )
    );

    let retried = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(&token)
        .json(&delete_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retried.status(), 200);
    assert_eq!(retried.json::<serde_json::Value>().await.unwrap(), deleted);

    let reused_for_update = client
        .post(format!("{}/app/v1/records/update", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": published["revision"],
            "mutation_id": "mut_record_delete",
            "data": br#"{"version":1,"value":{"text":"Wrong operation"}}"#.to_vec(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reused_for_update.status(), 400);
    assert_eq!(
        reused_for_update.json::<serde_json::Value>().await.unwrap()["code"],
        "invalid_input"
    );

    let stale = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": published["revision"],
            "mutation_id": "mut_stale_delete",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);
    assert_eq!(
        stale.json::<serde_json::Value>().await.unwrap()["code"],
        "record_conflict"
    );

    first_handle.shutdown().await.ok();
    let (second_port, second_handle, _second_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let retry_after_restart = client
        .post(format!("{}/app/v1/records/delete", base_url(second_port)))
        .bearer_auth(&token)
        .json(&delete_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retry_after_restart.status(), 200);
    assert_eq!(
        retry_after_restart
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        deleted
    );

    second_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_delete_record_allows_one_concurrent_same_revision_winner() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let path = "/chirp/posts/jlt_concurrent_delete";
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &["publish:/chirp/*", "delete:/chirp/*"],
    )
    .await;

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"delete once".to_vec()).file_name("record.txt"),
        )
        .text("path", path.to_string());
    let published = client
        .post(format!("{}/app/v1/publish", base_url(port)))
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 200);
    let published: serde_json::Value = published.json().await.unwrap();

    let first = client
        .post(format!("{}/app/v1/records/delete", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": published["revision"],
            "mutation_id": "mut_concurrent_delete_first",
        }))
        .send();
    let second = client
        .post(format!("{}/app/v1/records/delete", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": published["revision"],
            "mutation_id": "mut_concurrent_delete_second",
        }))
        .send();
    let (first, second) = tokio::join!(first, second);
    let mut statuses = [first.unwrap().status(), second.unwrap().status()];
    statuses.sort();

    assert_eq!(
        statuses,
        [reqwest::StatusCode::OK, reqwest::StatusCode::CONFLICT]
    );
    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_restore_record_is_publish_scoped_idempotent_and_durable() {
    let profile = tempfile::tempdir().unwrap();
    let (first_port, first_handle, _first_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let client = reqwest::Client::new();
    let identity = first_handle.status().await.unwrap().identity_address;
    let path = "/chirp/posts/jlt_restored_by_app";
    let token = approve_app_session(
        &client,
        first_port,
        &identity,
        &[
            "publish:/chirp/*",
            "delete:/chirp/*",
            "inventory:/chirp/*",
            "resolve:public",
            "fetch:public",
        ],
    )
    .await;

    let original = br#"{"version":1,"value":{"text":"Before delete"}}"#;
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(original.to_vec()).file_name("record.json"),
        )
        .text("path", path.to_string());
    let published = client
        .post(format!("{}/app/v1/publish", base_url(first_port)))
        .bearer_auth(&token)
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(published.status(), 200);
    let published: serde_json::Value = published.json().await.unwrap();

    let deleted = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": published["revision"],
            "mutation_id": "mut_delete_before_restore",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(deleted.status(), 200);
    let deleted: serde_json::Value = deleted.json().await.unwrap();

    let restored_bytes = br#"{"version":1,"value":{"text":"Restored"}}"#.to_vec();
    let restore_request = serde_json::json!({
        "path": path,
        "revision": deleted["revision"],
        "mutation_id": "mut_record_restore",
        "data": restored_bytes.to_vec(),
    });
    let no_publish_token =
        approve_app_session(&client, first_port, &identity, &["delete:/chirp/*"]).await;
    let denied = client
        .post(format!("{}/app/v1/records/restore", base_url(first_port)))
        .bearer_auth(no_publish_token)
        .json(&restore_request)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    let restored = client
        .post(format!("{}/app/v1/records/restore", base_url(first_port)))
        .bearer_auth(&token)
        .json(&restore_request)
        .send()
        .await
        .unwrap();
    assert_eq!(restored.status(), 200);
    let restored: serde_json::Value = restored.json().await.unwrap();
    assert_eq!(restored["path"], path);
    assert_eq!(restored["data"], serde_json::json!(restored_bytes));
    assert_ne!(restored["content_id"], published["content_id"]);
    assert_ne!(restored["revision"], deleted["revision"]);

    let read_restored = client
        .post(format!("{}/app/v1/records/read", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    assert_eq!(read_restored.status(), 200);
    assert_eq!(
        read_restored.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({
            "state": "present",
            "path": path,
            "content_id": restored["content_id"],
            "revision": restored["revision"],
            "data": restored_bytes,
        })
    );

    let retried = client
        .post(format!("{}/app/v1/records/restore", base_url(first_port)))
        .bearer_auth(&token)
        .json(&restore_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retried.status(), 200);
    assert_eq!(retried.json::<serde_json::Value>().await.unwrap(), restored);

    let reused_for_delete = client
        .post(format!("{}/app/v1/records/delete", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": restored["revision"],
            "mutation_id": "mut_record_restore",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reused_for_delete.status(), 400);
    assert_eq!(
        reused_for_delete.json::<serde_json::Value>().await.unwrap()["code"],
        "invalid_input"
    );

    let stale = client
        .post(format!("{}/app/v1/records/restore", base_url(first_port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": path,
            "revision": deleted["revision"],
            "mutation_id": "mut_stale_restore",
            "data": restored_bytes.to_vec(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(stale.status(), 409);
    assert_eq!(
        stale.json::<serde_json::Value>().await.unwrap()["code"],
        "record_conflict"
    );

    let inventory = client
        .get(format!("{}/app/v1/published", base_url(first_port)))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(inventory.status(), 200);
    assert!(inventory
        .json::<Vec<serde_json::Value>>()
        .await
        .unwrap()
        .iter()
        .any(|item| item["path"] == path && item["content_id"] == restored["content_id"]));

    first_handle.shutdown().await.ok();
    let (second_port, second_handle, _second_holder) =
        start_test_server_with_profile_dir(profile.path().to_path_buf()).await;
    let retry_after_restart = client
        .post(format!("{}/app/v1/records/restore", base_url(second_port)))
        .bearer_auth(&token)
        .json(&restore_request)
        .send()
        .await
        .unwrap();
    assert_eq!(retry_after_restart.status(), 200);
    assert_eq!(
        retry_after_restart
            .json::<serde_json::Value>()
            .await
            .unwrap(),
        restored
    );

    second_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_reads_local_tombstone_as_deleted_with_its_revision() {
    let dir = tempfile::tempdir().unwrap();
    let root = NodeIdentity::generate();
    let device = NodeIdentity::generate();
    let identity = root.identity_id();
    let path = "/chirp/posts/jlt_deleted";
    let authority = vec![device_authority_record(&root, &device, "dev_phone")];
    let present = DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_phone",
        DeviceWriterOperation::set_path(
            path,
            ContentId::from_bytes(b"post before delete"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| device.sign(bytes),
    )
    .unwrap();
    let tombstone = present
        .append(DeviceWriterOperation::tombstone_path(path), 101, |bytes| {
            device.sign(bytes)
        })
        .unwrap();
    let revision = tombstone.entry_hash().to_hex();
    let session_path = dir.path().join("app-sessions.json");
    let (port, handle, _dir) =
        start_test_server_with_identity_and_session_path(root, session_path, dir).await;
    handle
        .store_device_writer_logs(identity.clone(), authority, vec![vec![present, tombstone]])
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let session_identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &session_identity,
        &["resolve:public", "fetch:public"],
    )
    .await;
    let response = client
        .post(format!("{}/app/v1/records/read", base_url(port)))
        .bearer_auth(token)
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({
            "state": "deleted",
            "path": path,
            "revision": revision,
        })
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_any_enumeration_is_path_scoped_across_identities() {
    let (port, handle, _dir) = start_test_server().await;
    let session_identity = handle.status().await.unwrap().identity_address;
    let remote_root = NodeIdentity::generate();
    let remote_device = NodeIdentity::generate();
    let remote_identity = remote_root.identity_id();
    let content_id = ContentId::from_bytes(b"remote spoke record");
    let remote_log = vec![DeviceWriterLogEntry::genesis(
        remote_identity.clone(),
        "dev_remote",
        DeviceWriterOperation::set_path(
            "/spoke/posts/one",
            content_id,
            DeviceWriterPathMode::Append,
        ),
        100,
        |bytes| remote_device.sign(bytes),
    )
    .unwrap()];
    handle
        .store_device_writer_logs(
            remote_identity.clone(),
            vec![device_authority_record(
                &remote_root,
                &remote_device,
                "dev_remote",
            )],
            vec![remote_log],
        )
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let any_token = approve_app_session(
        &client,
        port,
        &session_identity,
        &["enumerate:any:/spoke/*"],
    )
    .await;
    let allowed = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&any_token)
        .json(&serde_json::json!({
            "identity": remote_identity.to_string(),
            "path_prefix": "/spoke/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(allowed.status(), 200);
    assert_eq!(
        allowed
            .json::<serde_json::Value>()
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let self_token = approve_app_session(
        &client,
        port,
        &session_identity,
        &["enumerate:self:/spoke/*"],
    )
    .await;
    let denied = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&self_token)
        .json(&serde_json::json!({
            "identity": remote_identity.to_string(),
            "path_prefix": "/spoke/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 403);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_encrypt_append_and_enumerate_records_by_prefix() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Tablet" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);

    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/app/private/*",
            "decrypt:/app/private/*",
            "publish:encrypted:/app/private/*",
            "enumerate:self:/app/private/*",
        ],
    )
    .await;

    let append_resp = client
        .post(format!("{}/app/v1/encrypted/append", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/app/private/items/1",
            "plaintext": b"private record one".to_vec(),
            "content_type": "application/json",
            "recipients": [identity]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(append_resp.status(), 200);
    let appended: serde_json::Value = append_resp.json().await.unwrap();
    assert_eq!(appended["path"], "/app/private/items/1");
    assert!(appended["content_id"].as_str().unwrap().len() > 0);
    assert_eq!(appended["recipient_count"], 2);

    let enumerated = client
        .post(format!("{}/app/v1/enumerate", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "identity": identity,
            "path_prefix": "/app/private/items/",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(enumerated.status(), 200);
    let records: serde_json::Value = enumerated.json().await.unwrap();
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["path"], "/app/private/items/1");
    assert_eq!(records[0]["content_id"], appended["content_id"]);

    let open_resp = client
        .post(format!("{}/app/v1/encrypted/open", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "target": appended["content_id"],
            "path": records[0]["path"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["status"], "decrypted");
    assert_eq!(opened["content_id"], appended["content_id"]);
    assert_eq!(opened["path"], "/app/private/items/1");
    assert_eq!(opened["content_type"], "application/json");
    let plaintext: Vec<u8> = opened["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"private record one");

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
async fn test_app_can_encrypt_publish_and_decrypt_scoped_content() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/private",
            "plaintext": b"private paste".to_vec(),
            "content_type": "text/plain",
            "recipients": [identity]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["path"], "/pastes/private");
    assert_eq!(published["address"], format!("{identity}/pastes/private"));
    assert!(published["size"].as_u64().unwrap() > b"private paste".len() as u64);

    let public_fetch = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": published["content_id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(public_fetch.status(), 200);
    let public_body: serde_json::Value = public_fetch.json().await.unwrap();
    let ciphertext: Vec<u8> = public_body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_ne!(ciphertext, b"private paste");

    let decrypt_resp = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(decrypt_resp.status(), 200);
    let decrypted: serde_json::Value = decrypt_resp.json().await.unwrap();
    assert_eq!(decrypted["content_id"], published["content_id"]);
    assert_eq!(decrypted["path"], "/pastes/private");
    let plaintext: Vec<u8> = decrypted["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"private paste");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_encrypt_publish_self_only_private_content() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/private-note",
            "plaintext": b"note to self".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["path"], "/pastes/private-note");
    assert_eq!(
        published["address"],
        format!("{identity}/pastes/private-note")
    );
    assert_eq!(published["recipient_count"], 1);

    let decrypt_resp = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(decrypt_resp.status(), 200);
    let decrypted: serde_json::Value = decrypt_resp.json().await.unwrap();
    let plaintext: Vec<u8> = decrypted["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"note to self");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_encrypts_self_private_content_for_active_authorized_devices() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);

    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/device-wrapped",
            "plaintext": b"follows future devices".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["recipient_count"], 2);

    let fetch_resp = client
        .post(format!("{}/api/v1/fetch", base_url(port)))
        .json(&serde_json::json!({ "target": published["content_id"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(fetch_resp.status(), 200);
    let fetched: serde_json::Value = fetch_resp.json().await.unwrap();
    let envelope_bytes: Vec<u8> = fetched["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    let envelope = EncryptedObjectEnvelope::from_bytes(&envelope_bytes).unwrap();
    assert_eq!(envelope.body.recipients.len(), 2);
    let recipient_identity = JoltAddress::from_str(&identity).unwrap().identity().clone();
    assert!(envelope
        .body
        .recipients
        .iter()
        .all(|recipient| recipient.recipient_identity == recipient_identity));

    let decrypt_resp = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(decrypt_resp.status(), 200);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_open_reports_historical_private_content_needs_rewrap() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/historical",
            "plaintext": b"before phone auth".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["recipient_count"], 1);

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);

    let open_resp = client
        .post(format!("{}/app/v1/encrypted/open", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "target": published["content_id"],
            "path": "/pastes/historical"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["status"], "decrypted");
    assert_eq!(opened["access_status"], "needs_rewrap");
    let plaintext: Vec<u8> = opened["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"before phone auth");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_rewrap_historical_private_content_for_authorized_devices() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/rewrap-me",
            "plaintext": b"rewrap this private paste".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let original: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(original["recipient_count"], 1);

    let authorize_resp = client
        .post(format!(
            "{}/admin/v1/device-authority/devices",
            base_url(port)
        ))
        .json(&serde_json::json!({ "label": "Phone" }))
        .send()
        .await
        .unwrap();
    assert_eq!(authorize_resp.status(), 200);

    let rewrap_resp = client
        .post(format!("{}/app/v1/encrypted/rewrap", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "target": original["content_id"],
            "path": "/pastes/rewrap-me"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(rewrap_resp.status(), 200);
    let rewrapped: serde_json::Value = rewrap_resp.json().await.unwrap();
    assert_eq!(rewrapped["path"], "/pastes/rewrap-me");
    assert_ne!(rewrapped["content_id"], original["content_id"]);
    assert_eq!(rewrapped["recipient_count"], 2);

    let open_resp = client
        .post(format!("{}/app/v1/encrypted/open", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": rewrapped["address"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["status"], "decrypted");
    assert_eq!(opened["access_status"], "available");
    let plaintext: Vec<u8> = opened["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"rewrap this private paste");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_open_encrypted_content_with_one_daemon_call() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/pastes/private-open",
            "plaintext": b"open once".to_vec(),
            "content_type": "text/plain",
            "recipients": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();

    let open_resp = client
        .post(format!("{}/app/v1/encrypted/open", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["status"], "decrypted");
    assert_eq!(opened["content_id"], published["content_id"]);
    assert_eq!(opened["path"], "/pastes/private-open");
    assert_eq!(opened["content_type"], "text/plain");
    assert!(opened["ciphertext"].is_null());
    assert!(opened["decrypt_error"].is_null());
    let plaintext: Vec<u8> = opened["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, b"open once");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_open_encrypted_content_returns_ciphertext_for_non_recipient() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    handle.ensure_local_identity_encryption_key().await.unwrap();

    let author = NodeIdentity::generate();
    let recipient_identity = NodeIdentity::generate().identity_id();
    let (recipient_key, _recipient_private_key) = generate_identity_encryption_keypair(
        recipient_identity.clone(),
        "enc_x25519_other_v0".to_string(),
        1,
    );
    let envelope = EncryptedObjectEnvelope::encrypt(
        author.public_key_bytes(),
        author.identity_id(),
        b"not for this daemon",
        "text/plain".to_string(),
        None,
        vec![EncryptedObjectRecipient {
            identity: recipient_identity,
            key: recipient_key,
        }],
        1,
        |bytes| author.sign(bytes),
    )
    .unwrap();
    let envelope_bytes = envelope.to_bytes().unwrap();

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(envelope_bytes.clone()).file_name("encrypted.json"),
        )
        .text("path", "/pastes/not-for-me");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();

    let token = approve_app_session(&client, port, &identity, &["decrypt:/pastes/*"]).await;
    let open_resp = client
        .post(format!("{}/app/v1/encrypted/open", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["status"], "ciphertext");
    assert_eq!(opened["access_status"], "not_accessible");
    assert_eq!(opened["content_id"], published["content_id"]);
    assert_eq!(opened["path"], "/pastes/not-for-me");
    assert!(opened["plaintext"].is_null());
    assert!(opened["decrypt_error"]
        .as_str()
        .unwrap()
        .contains("recipient"));
    let ciphertext: Vec<u8> = opened["ciphertext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(ciphertext, envelope_bytes);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_encrypted_publish_and_decrypt_enforce_path_capabilities() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    let publish_token = approve_app_session(
        &client,
        port,
        &identity,
        &[
            "encrypt:/pastes/*",
            "decrypt:/pastes/*",
            "publish:encrypted:/pastes/*",
        ],
    )
    .await;

    let denied_publish = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&publish_token)
        .json(&serde_json::json!({
            "path": "/notes/private",
            "plaintext": b"outside prefix".to_vec(),
            "recipients": [identity]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_publish.status(), 403);

    let publish_resp = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&publish_token)
        .json(&serde_json::json!({
            "path": "/pastes/private",
            "plaintext": b"private paste".to_vec(),
            "recipients": [identity]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();

    let no_decrypt_token = approve_app_session(
        &client,
        port,
        &identity,
        &["encrypt:/pastes/*", "publish:encrypted:/pastes/*"],
    )
    .await;
    let missing_decrypt = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&no_decrypt_token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing_decrypt.status(), 403);

    let wrong_path_token =
        approve_app_session(&client, port, &identity, &["decrypt:/notes/*"]).await;
    let outside_decrypt = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&wrong_path_token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(outside_decrypt.status(), 403);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_decrypt_fails_when_local_identity_is_not_a_recipient() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let identity = handle.status().await.unwrap().identity_address;
    handle.ensure_local_identity_encryption_key().await.unwrap();

    let author = NodeIdentity::generate();
    let recipient_identity = NodeIdentity::generate().identity_id();
    let (recipient_key, _recipient_private_key) = generate_identity_encryption_keypair(
        recipient_identity.clone(),
        "enc_x25519_other_v0".to_string(),
        1,
    );
    let envelope = EncryptedObjectEnvelope::encrypt(
        author.public_key_bytes(),
        author.identity_id(),
        b"not for this daemon",
        "text/plain".to_string(),
        None,
        vec![EncryptedObjectRecipient {
            identity: recipient_identity,
            key: recipient_key,
        }],
        1,
        |bytes| author.sign(bytes),
    )
    .unwrap();
    let envelope_bytes = envelope.to_bytes().unwrap();

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(envelope_bytes).file_name("encrypted.json"),
        )
        .text("path", "/pastes/not-for-me");
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();

    let token = approve_app_session(&client, port, &identity, &["decrypt:/pastes/*"]).await;
    let decrypt_resp = client
        .post(format!("{}/app/v1/encrypted/decrypt", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({ "target": published["address"] }))
        .send()
        .await
        .unwrap();

    assert_eq!(decrypt_resp.status(), 400);
    let body: serde_json::Value = decrypt_resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_input");
    assert!(body["error"].as_str().unwrap().contains("recipient"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_encrypts_for_verified_remote_recipient_identity() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let local_identity = handle.status().await.unwrap().identity_address;
    let token = approve_app_session(
        &client,
        port,
        &local_identity,
        &["encrypt:/shares/*", "publish:encrypted:/shares/*"],
    )
    .await;

    let remote = NodeIdentity::generate();
    let remote_identity = remote.identity_id();
    let (remote_key, _remote_private_key) = generate_identity_encryption_keypair(
        remote_identity.clone(),
        "enc_remote_v0".to_string(),
        1,
    );
    let key_record = IdentityEncryptionKeyRecord::new(
        remote.public_key_bytes(),
        remote_identity.clone(),
        vec![remote_key],
        0,
        1,
        |bytes| remote.sign(bytes),
    )
    .unwrap();
    let record_json = serde_json::to_vec(&key_record).unwrap();
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(record_json).file_name("remote-keys.json"),
    );
    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);
    let published_record: serde_json::Value = publish_resp.json().await.unwrap();
    let key_record_content_id: ContentId = published_record["content_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let remote_update_log = vec![UpdateLogEntry::genesis(
        remote.public_key_bytes(),
        UpdateAction::SetPath {
            path: IDENTITY_ENCRYPTION_KEYS_PATH.to_string(),
            content_id: key_record_content_id,
        },
        |bytes| remote.sign(bytes),
    )
    .unwrap()];
    handle
        .store_update_log(remote_identity.clone(), remote_update_log)
        .await
        .unwrap();

    let remote_address = JoltAddress::new(remote_identity, "/").unwrap().to_string();
    let publish_encrypted = client
        .post(format!("{}/app/v1/encrypted/publish", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "path": "/shares/remote",
            "plaintext": b"for a remote recipient".to_vec(),
            "recipients": [remote_address]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_encrypted.status(), 200);
    let encrypted: serde_json::Value = publish_encrypted.json().await.unwrap();
    assert_eq!(encrypted["path"], "/shares/remote");
    assert_eq!(encrypted["recipient_count"], 2);

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
    let identity_dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    identity.save(identity_dir.path()).unwrap();
    let first_identity = NodeIdentity::load(identity_dir.path()).unwrap();
    let local_identity = first_identity.jolt_address().to_string();
    let first_store_dir = tempfile::tempdir().unwrap();
    let (first_port, first_handle, _first_dir) = start_test_server_with_identity_and_session_path(
        first_identity,
        session_path.clone(),
        first_store_dir,
    )
    .await;
    let client = reqwest::Client::new();

    let request_resp = client
        .post(format!("{}/app/v1/sessions/request", base_url(first_port)))
        .json(&serde_json::json!({
            "app_id": "pastey.local",
            "app_name": "Pastey",
            "requested_identity": local_identity,
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
            "identity": local_identity,
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

    let second_identity = NodeIdentity::load(identity_dir.path()).unwrap();
    let second_store_dir = tempfile::tempdir().unwrap();
    let (second_port, second_handle, _second_dir) =
        start_test_server_with_identity_and_session_path(
            second_identity,
            session_path,
            second_store_dir,
        )
        .await;
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
    assert_eq!(sessions[0]["identity"], local_identity);
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
    assert_eq!(body["daemon_version"], env!("CARGO_PKG_VERSION"));
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
async fn test_app_api_feature_manifest_is_public_and_generic() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/app/v1/features", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["app_api"], 1);
    assert_eq!(body["features"], serde_json::json!({ "data.records": 4 }));
    assert!(body.get("daemon_version").is_none());
    assert!(body.get("applications").is_none());

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
async fn test_admin_network_settings_can_update_bootstrap_and_home_relay() {
    let settings_dir = tempfile::tempdir().unwrap();
    let settings_path = settings_dir.path().join("config.json");
    std::fs::write(&settings_path, r#"{ "future_setting": "keep-me" }"#).unwrap();
    let (port, handle, _dir) =
        start_test_server_with_network_settings_path(settings_path.clone()).await;
    let client = reqwest::Client::new();
    let relay =
        "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

    let initial = client
        .get(format!("{}/admin/v1/network-settings", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(initial.status(), 200);
    let initial: serde_json::Value = initial.json().await.unwrap();
    assert!(initial["configured_bootstrap_relays"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(initial["built_in_bootstrap_relays"].is_array());
    assert!(initial["effective_bootstrap_relays"].is_array());

    let added = client
        .post(format!("{}/admin/v1/bootstrap-relays", base_url(port)))
        .json(&serde_json::json!({ "multiaddr": relay }))
        .send()
        .await
        .unwrap();
    assert_eq!(added.status(), 200);
    let added: serde_json::Value = added.json().await.unwrap();
    assert_eq!(added["configured_bootstrap_relays"][0], relay);
    assert_eq!(added["effective_bootstrap_relays"][0], relay);

    let invalid_bootstrap = client
        .post(format!("{}/admin/v1/bootstrap-relays", base_url(port)))
        .json(&serde_json::json!({ "multiaddr": "/ip4/127.0.0.1/tcp/4001" }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_bootstrap.status(), 400);
    let invalid_bootstrap: serde_json::Value = invalid_bootstrap.json().await.unwrap();
    assert_eq!(invalid_bootstrap["code"], "invalid_network_settings");

    let app_bootstrap = client
        .post(format!("{}/app/v1/bootstrap-relays", base_url(port)))
        .json(&serde_json::json!({ "multiaddr": relay }))
        .send()
        .await
        .unwrap();
    assert_eq!(app_bootstrap.status(), 404);

    let invalid_home_relay = client
        .post(format!("{}/admin/v1/home-relay", base_url(port)))
        .json(&serde_json::json!({
            "multiaddr": relay,
            "capability": "pinning",
            "api_url": "not a url"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(invalid_home_relay.status(), 400);

    let home_relay = client
        .post(format!("{}/admin/v1/home-relay", base_url(port)))
        .json(&serde_json::json!({
            "multiaddr": relay,
            "capability": "pinning",
            "api_url": "http://127.0.0.1:9870"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(home_relay.status(), 200);
    let home_relay: serde_json::Value = home_relay.json().await.unwrap();
    assert_eq!(home_relay["home_relay"]["multiaddr"], relay);
    assert_eq!(home_relay["home_relay"]["api_url"], "http://127.0.0.1:9870");

    let cleared = client
        .post(format!("{}/admin/v1/home-relay/clear", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), 200);
    let cleared: serde_json::Value = cleared.json().await.unwrap();
    assert!(cleared["home_relay"].is_null());

    let removed = client
        .post(format!(
            "{}/admin/v1/bootstrap-relays/remove",
            base_url(port)
        ))
        .json(&serde_json::json!({ "multiaddr": relay }))
        .send()
        .await
        .unwrap();
    assert_eq!(removed.status(), 200);
    let removed: serde_json::Value = removed.json().await.unwrap();
    assert!(removed["configured_bootstrap_relays"]
        .as_array()
        .unwrap()
        .is_empty());

    let persisted: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(settings_path).unwrap()).unwrap();
    assert_eq!(persisted["future_setting"], "keep-me");

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
async fn test_admin_relay_status_reports_enabled_relay_shape() {
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
        .get(format!("{}/admin/v1/relay/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["relay_enabled"], true);
    assert_eq!(
        body["peer_id"].as_str(),
        body["relay_record"]["body"]["peer_id"].as_str()
    );
    assert!(body["listen_addresses"][0]
        .as_str()
        .unwrap()
        .contains(&format!("/tcp/{p2p_port}")));
    assert_eq!(body["bootstrap"]["state"], "disconnected");
    assert_eq!(body["bootstrap"]["connected_peer_count"], 0);
    assert_eq!(body["bootstrap"]["effective_relay_count"], 0);
    assert_eq!(body["peers"]["connected"], 0);
    assert_eq!(body["peers"]["direct"], 0);
    assert_eq!(body["peers"]["relayed"], 0);
    assert_eq!(body["known_relay_count"], 0);
    assert_eq!(body["cache"]["pinned_items"], 0);
    assert_eq!(
        body["relay_record"]["body"]["capabilities"],
        serde_json::json!(["Bootstrap", "Discovery", "Pinning"])
    );

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_relay_status_reports_disabled_relay_shape() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/admin/v1/relay/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["relay_enabled"], false);
    assert!(body["relay_record"].is_null());
    assert_eq!(body["bootstrap"]["state"], "disconnected");
    assert_eq!(body["cache"]["pinned_items"], 0);
    assert!(body["home_relay"].is_null());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_relay_status_is_not_exposed_through_app_api() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{}/app/v1/relay/status", base_url(port)))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

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
async fn test_resolve_endpoint_uses_verified_device_writer_cache() {
    let (port, handle, _dir) = start_test_server().await;
    let root = NodeIdentity::generate();
    let laptop = NodeIdentity::generate();
    let identity = root.identity_id();
    let device_id = "dev_laptop";
    let authority = vec![device_authority_record(&root, &laptop, device_id)];
    let content_id = ContentId::from_bytes(b"device writer profile via api");
    let device_log = vec![DeviceWriterLogEntry::genesis(
        identity.clone(),
        device_id,
        DeviceWriterOperation::set_path(
            "/profile",
            content_id.clone(),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| laptop.sign(bytes),
    )
    .unwrap()];
    let address = JoltAddress::new(identity.clone(), "/profile").unwrap();

    handle
        .store_device_writer_logs(identity.clone(), authority, vec![device_log])
        .await
        .unwrap();

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
    assert_eq!(body["identity"], identity.to_string());
    assert_eq!(body["path"], "/profile");
    assert_eq!(body["latest_sequence"], 0);
    assert_eq!(body["content_id"], content_id.to_string());
    assert_eq!(body["reachability_hints"].as_array().unwrap().len(), 0);
    assert_eq!(body["source"], "device_writer_cache");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_resolve_endpoint_returns_structured_path_tombstoned_error() {
    let (port, handle, _dir) = start_test_server().await;
    let root = NodeIdentity::generate();
    let device = NodeIdentity::generate();
    let identity = root.identity_id();
    let path = "/profile";
    let present = DeviceWriterLogEntry::genesis(
        identity.clone(),
        "dev_phone",
        DeviceWriterOperation::set_path(
            path,
            ContentId::from_bytes(b"profile before delete"),
            DeviceWriterPathMode::Singleton,
        ),
        100,
        |bytes| device.sign(bytes),
    )
    .unwrap();
    let tombstone = present
        .append(DeviceWriterOperation::tombstone_path(path), 101, |bytes| {
            device.sign(bytes)
        })
        .unwrap();
    handle
        .store_device_writer_logs(
            identity.clone(),
            vec![device_authority_record(&root, &device, "dev_phone")],
            vec![vec![present, tombstone]],
        )
        .await
        .unwrap();

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/resolve", base_url(port)))
        .json(&serde_json::json!({
            "address": JoltAddress::new(identity, path).unwrap().to_string(),
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 410);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({
            "error": format!("Path is tombstoned: {path}"),
            "code": "path_tombstoned",
        })
    );

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}")
        .parse()
        .unwrap();

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");
    let relay_addr: Multiaddr = relay_multiaddr.parse().unwrap();

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

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

    let alice_identity = NodeIdentity::generate();
    let relay_identity = NodeIdentity::generate();
    let relay_store = ContentStore::open(relay_dir.path(), CacheConfig::default()).unwrap();
    let mut relay = NetworkNode::new_tcp(
        relay_identity,
        relay_store,
        relay_config_for(&alice_identity),
    )
    .unwrap();
    relay
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{relay_p2p}"))
        .unwrap();
    let (relay_api, relay_handle, _relay_dir) = start_test_server_from_node(relay, relay_dir).await;
    let relay_peer = relay_handle.status().await.unwrap().peer_id;
    let relay_multiaddr = format!("/ip4/127.0.0.1/tcp/{relay_p2p}/p2p/{relay_peer}");

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
async fn test_relay_pin_request_is_denied_when_allowlist_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut node = NetworkNode::new_tcp(NodeIdentity::generate(), store, relay_config()).unwrap();
    node.set_fetch_timeout(std::time::Duration::from_millis(20));
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;

    let owner = NodeIdentity::generate();
    let request = PinRequest::new(
        owner.public_key_bytes(),
        ContentId::from_bytes(b"content denied before fetch"),
        |bytes| owner.sign(bytes),
    )
    .unwrap();
    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/relay/pins", base_url(port)))
        .json(&request)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["error"],
        "relay pin denied: identity is not allowlisted"
    );
    assert_eq!(handle.cache_stats().await.unwrap().pinned_items, 0);

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
async fn test_admin_relay_diagnose_identity_reports_no_bootstrap_relays() {
    let (port, handle, _dir) = start_test_server().await;
    let identity = NodeIdentity::generate().identity_id();
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/admin/v1/relay/diagnose/identity",
            base_url(port)
        ))
        .json(&serde_json::json!({ "identity": identity.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["identity"], identity.to_string());
    assert_eq!(body["provider_key"], format!("jolt:update-log:{identity}"));
    assert_eq!(body["local_update_log_cache"]["state"], "miss");
    assert_eq!(body["identity_head_hint"]["state"], "miss");
    assert_eq!(body["local_provider_candidates"], serde_json::json!([]));
    assert_eq!(body["provider_candidates"], serde_json::json!([]));
    assert_eq!(body["relay_forwarding"]["attempted"], false);
    assert_eq!(body["relay_forwarding"]["target_count"], 0);
    assert_eq!(body["outcome"]["code"], "no_bootstrap_relays");
    assert!(body["outcome"]["message"]
        .as_str()
        .unwrap()
        .contains("no bootstrap relays"));

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_relay_diagnose_identity_reports_local_provider_candidate() {
    let (port, handle, _dir) = start_test_server().await;
    let owner = NodeIdentity::generate();
    let identity = owner.identity_id();
    handle
        .store_update_log(identity.clone(), signed_profile_log(&owner, b"profile"))
        .await
        .unwrap();
    let local_peer_id = handle.status().await.unwrap().peer_id;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!(
            "{}/admin/v1/relay/diagnose/identity",
            base_url(port)
        ))
        .json(&serde_json::json!({ "identity": identity.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["identity"], identity.to_string());
    assert_eq!(body["local_update_log_cache"]["state"], "hit");
    assert_eq!(body["local_update_log_cache"]["entry_count"], 1);
    assert_eq!(body["local_update_log_cache"]["latest_sequence"], 0);
    assert_eq!(body["provider_candidates"][0]["peer_id"], local_peer_id);
    assert_eq!(
        body["local_provider_candidates"][0]["peer_id"],
        local_peer_id
    );
    assert_eq!(body["outcome"]["code"], "provider_candidates_found");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_relay_diagnose_identity_reports_forwarded_no_candidates() {
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
    alice_config.effective_bootstrap_relays = vec![relay_multiaddr];
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, alice_config).unwrap();
    alice
        .listen_on(&format!("/ip4/127.0.0.1/tcp/{alice_p2p}"))
        .unwrap();
    alice.bootstrap_dht(&[relay_addr]).unwrap();
    let (alice_api, alice_handle, _alice_dir) = start_test_server_from_node(alice, alice_dir).await;
    wait_for_connected_peers(&alice_handle, 1).await;

    let missing_identity = NodeIdentity::generate().identity_id();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/admin/v1/relay/diagnose/identity",
            base_url(alice_api)
        ))
        .json(&serde_json::json!({ "identity": missing_identity.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["outcome"]["code"], "identity_provider_not_found");
    assert_eq!(body["relay_forwarding"]["attempted"], true);
    assert_eq!(body["relay_forwarding"]["target_count"], 1);
    assert_eq!(
        body["relay_forwarding"]["attempts"][0]["peer_id"],
        relay_peer
    );
    assert_eq!(
        body["relay_forwarding"]["attempts"][0]["status"],
        "responded"
    );
    assert_eq!(
        body["relay_forwarding"]["attempts"][0]["candidate_count"],
        0
    );
    assert_eq!(body["provider_candidates"], serde_json::json!([]));

    relay_handle.shutdown().await.ok();
    alice_handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_relay_diagnose_identity_is_not_exposed_through_app_api() {
    let (port, handle, _dir) = start_test_server().await;
    let identity = NodeIdentity::generate().identity_id();
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/app/v1/relay/diagnose/identity", base_url(port)))
        .json(&serde_json::json!({ "identity": identity.to_string() }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);

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
    assert_eq!(resolved["source"], "device_writer_cache");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_identity_encryption_keys_endpoint_returns_verified_record_keys() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let identity_label = identity.identity_id().to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let record = IdentityEncryptionKeyRecord::new(
        identity.public_key_bytes(),
        identity.identity_id(),
        vec![IdentityEncryptionKey {
            key_id: "enc_x25519_test".to_string(),
            suite_family: "x25519-hkdf-sha256".to_string(),
            key_type: "OKP".to_string(),
            curve: "X25519".to_string(),
            public_key: vec![11; 32],
            created_at: now,
            not_before: now.saturating_sub(60),
            expires_at: Some(now + 3600),
            status: "active".to_string(),
        }],
        7,
        now,
        |bytes| identity.sign(bytes),
    )
    .unwrap();
    let record_json = serde_json::to_vec(&record).unwrap();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();
    let (port, handle, _dir) = start_test_server_from_node(node, dir).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(record_json).file_name("encryption-keys.json"),
        )
        .text("path", IDENTITY_ENCRYPTION_KEYS_PATH.to_string());

    let publish_resp = client
        .post(format!("{}/api/v1/publish", base_url(port)))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(publish_resp.status(), 200);

    let keys_resp = client
        .get(format!(
            "{}/api/v1/identities/{identity_label}/encryption-keys",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(keys_resp.status(), 200);
    let body: serde_json::Value = keys_resp.json().await.unwrap();
    assert_eq!(body["identity"], identity_label);
    assert_eq!(body["latest_sequence"], 7);
    assert_eq!(body["keys"].as_array().unwrap().len(), 1);
    assert_eq!(body["keys"][0]["key_id"], "enc_x25519_test");
    assert_eq!(body["keys"][0]["suite_family"], "x25519-hkdf-sha256");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_admin_can_publish_and_api_can_verify_signed_reachability() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let status = handle.status().await.unwrap();
    let identity_label = status
        .identity_address
        .trim_end_matches('/')
        .strip_suffix(".jolt")
        .unwrap()
        .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let publish_resp = client
        .post(format!("{}/admin/v1/reachability", base_url(port)))
        .json(&serde_json::json!({
            "sequence_hint": 12,
            "expires_at": now + 3600,
            "live": [{
                "transport": "jolt-libp2p-stream",
                "peer_id": "12D3KooWReachablePeer",
                "addresses": ["/ip4/127.0.0.1/udp/4100/quic-v1"],
                "relay_hints": [],
                "protocols": ["opaque-app-stream-v1"],
                "max_payload_bytes": 65536
            }],
            "offline_ingress": []
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(publish_resp.status(), 200);
    let published: serde_json::Value = publish_resp.json().await.unwrap();
    assert_eq!(published["identity"], identity_label);
    assert_eq!(published["path"], SIGNED_REACHABILITY_PATH);
    assert_eq!(published["record"]["sequence_hint"], 12);
    assert_eq!(published["record"]["live"].as_array().unwrap().len(), 1);

    let get_resp = client
        .get(format!(
            "{}/api/v1/identities/{identity_label}/reachability",
            base_url(port)
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(get_resp.status(), 200);
    let body: serde_json::Value = get_resp.json().await.unwrap();
    assert_eq!(body["identity"], identity_label);
    assert_eq!(body["sequence_hint"], 12);
    assert_eq!(body["live"][0]["transport"], "jolt-libp2p-stream");
    assert_eq!(body["live"][0]["protocols"][0], "opaque-app-stream-v1");
    assert_eq!(body["offline_ingress"].as_array().unwrap().len(), 0);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_recipient_ingress_allows_browser_preflight() {
    let (port, _handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();

    let preflight_resp = client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/api/v1/ingress", base_url(port)),
        )
        .header(reqwest::header::ORIGIN, "http://127.0.0.1:5179")
        .header(reqwest::header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
        .header(
            reqwest::header::ACCESS_CONTROL_REQUEST_HEADERS,
            "content-type",
        )
        .send()
        .await
        .unwrap();

    assert_eq!(preflight_resp.status(), 200);
    assert_eq!(
        preflight_resp
            .headers()
            .get(reqwest::header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .unwrap(),
        "*"
    );
}

#[tokio::test]
async fn test_recipient_ingress_submit_list_and_reject() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let (bob_identity, encrypted_object) = encrypted_spoke_reply_for_local_identity(&handle).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let submit_resp = client
        .post(format!("{}/api/v1/ingress", base_url(port)))
        .json(&serde_json::json!({
            "receiver_id": "direct-local",
            "encrypted_object": encrypted_object,
            "expires_at": now + 3600
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(submit_resp.status(), 200);
    let submitted: serde_json::Value = submit_resp.json().await.unwrap();
    let ingress_id = submitted["ingress_id"].as_str().unwrap();
    assert_eq!(submitted["status"], "pending");
    assert_eq!(
        submitted["recipient_identity"],
        bob_identity.trim_end_matches(".jolt")
    );
    assert_eq!(submitted["schema_hint"], "application/vnd.spoke.reply+json");

    let app_token = approve_app_session(
        &client,
        port,
        &bob_identity,
        &["ingress:read", "ingress:decide"],
    )
    .await;

    let pending_resp = client
        .get(format!("{}/app/v1/ingress/pending", base_url(port)))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();

    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    let pending_items = pending.as_array().unwrap();
    assert_eq!(pending_items.len(), 1);
    assert_eq!(pending_items[0]["ingress_id"], ingress_id);
    assert_eq!(pending_items[0]["status"], "pending");
    assert_eq!(
        pending_items[0]["schema_hint"],
        "application/vnd.spoke.reply+json"
    );

    let open_resp = client
        .post(format!(
            "{}/app/v1/ingress/{ingress_id}/open",
            base_url(port)
        ))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();
    assert_eq!(open_resp.status(), 200);
    let opened: serde_json::Value = open_resp.json().await.unwrap();
    assert_eq!(opened["content_type"], "application/json");
    let plaintext: Vec<u8> = opened["plaintext"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u8)
        .collect();
    assert_eq!(plaintext, br#"{"post":"bob/post/1","body":"hello"}"#);

    let reject_resp = client
        .post(format!(
            "{}/app/v1/ingress/{ingress_id}/reject",
            base_url(port)
        ))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();

    assert_eq!(reject_resp.status(), 200);
    let rejected: serde_json::Value = reject_resp.json().await.unwrap();
    assert_eq!(rejected["ingress_id"], ingress_id);
    assert_eq!(rejected["status"], "rejected");

    let pending_after_reject_resp = client
        .get(format!("{}/app/v1/ingress/pending", base_url(port)))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();
    assert_eq!(pending_after_reject_resp.status(), 200);
    let pending_after_reject: serde_json::Value = pending_after_reject_resp.json().await.unwrap();
    assert!(pending_after_reject.as_array().unwrap().is_empty());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_app_can_submit_ingress_by_identity_reachability() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let (identity, encrypted_object) = encrypted_spoke_reply_for_local_identity(&handle).await;
    let identity_label = identity.trim_end_matches(".jolt");
    let token =
        approve_app_session(&client, port, &identity, &["ingress:send", "ingress:read"]).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let publish_reachability_resp = client
        .post(format!("{}/admin/v1/reachability", base_url(port)))
        .json(&serde_json::json!({
            "sequence_hint": 20,
            "expires_at": now + 3600,
            "live": [{
                "transport": "jolt-http-ingress",
                "peer_id": handle.status().await.unwrap().peer_id,
                "addresses": [format!("{}/api/v1/ingress", base_url(port))],
                "relay_hints": [],
                "protocols": ["recipient-ingress-v1"],
                "max_payload_bytes": 65536
            }],
            "offline_ingress": []
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(publish_reachability_resp.status(), 200);

    let send_resp = client
        .post(format!("{}/app/v1/ingress/send", base_url(port)))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "recipient": identity_label,
            "encrypted_object": encrypted_object,
            "expires_at": now + 600
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(send_resp.status(), 200);
    let sent: serde_json::Value = send_resp.json().await.unwrap();
    assert_eq!(sent["recipient_identity"], identity_label);
    assert_eq!(sent["status"], "pending");

    let pending_resp = client
        .get(format!("{}/app/v1/ingress/pending", base_url(port)))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    assert_eq!(pending.as_array().unwrap().len(), 1);
    assert_eq!(pending[0]["ingress_id"], sent["ingress_id"]);

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_recipient_ingress_accept_marks_pending_object_accepted() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let (bob_identity, encrypted_object) = encrypted_spoke_reply_for_local_identity(&handle).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let submit_resp = client
        .post(format!("{}/api/v1/ingress", base_url(port)))
        .json(&serde_json::json!({
            "receiver_id": "direct-local",
            "encrypted_object": encrypted_object,
            "expires_at": now + 3600
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(submit_resp.status(), 200);
    let submitted: serde_json::Value = submit_resp.json().await.unwrap();
    let ingress_id = submitted["ingress_id"].as_str().unwrap();

    let app_token = approve_app_session(
        &client,
        port,
        &bob_identity,
        &["ingress:read", "ingress:decide"],
    )
    .await;

    let accept_resp = client
        .post(format!(
            "{}/app/v1/ingress/{ingress_id}/accept",
            base_url(port)
        ))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();

    assert_eq!(accept_resp.status(), 200);
    let accepted: serde_json::Value = accept_resp.json().await.unwrap();
    assert_eq!(accepted["ingress_id"], ingress_id);
    assert_eq!(accepted["status"], "accepted");
    assert!(accepted["accepted_at"].as_u64().is_some());

    let repeated_accept_resp = client
        .post(format!(
            "{}/app/v1/ingress/{ingress_id}/accept",
            base_url(port)
        ))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();

    assert_eq!(repeated_accept_resp.status(), 200);
    let repeated_accept: serde_json::Value = repeated_accept_resp.json().await.unwrap();
    assert_eq!(repeated_accept["ingress_id"], ingress_id);
    assert_eq!(repeated_accept["status"], "accepted");

    let pending_resp = client
        .get(format!("{}/app/v1/ingress/pending", base_url(port)))
        .bearer_auth(&app_token)
        .send()
        .await
        .unwrap();
    assert_eq!(pending_resp.status(), 200);
    let pending: serde_json::Value = pending_resp.json().await.unwrap();
    assert!(pending.as_array().unwrap().is_empty());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_recipient_ingress_app_review_requires_session_and_capability() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let (bob_identity, encrypted_object) = encrypted_spoke_reply_for_local_identity(&handle).await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let submit_resp = client
        .post(format!("{}/api/v1/ingress", base_url(port)))
        .json(&serde_json::json!({
            "receiver_id": "direct-local",
            "encrypted_object": encrypted_object,
            "expires_at": now + 3600
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(submit_resp.status(), 200);
    let submitted: serde_json::Value = submit_resp.json().await.unwrap();
    let ingress_id = submitted["ingress_id"].as_str().unwrap();

    let no_token_resp = client
        .get(format!("{}/app/v1/ingress/pending", base_url(port)))
        .send()
        .await
        .unwrap();
    assert_eq!(no_token_resp.status(), 401);

    let read_only_token =
        approve_app_session(&client, port, &bob_identity, &["ingress:read"]).await;
    let reject_resp = client
        .post(format!(
            "{}/app/v1/ingress/{ingress_id}/reject",
            base_url(port)
        ))
        .bearer_auth(&read_only_token)
        .send()
        .await
        .unwrap();
    assert_eq!(reject_resp.status(), 403);
    let body: serde_json::Value = reject_resp.json().await.unwrap();
    assert_eq!(body["code"], "app_session_forbidden");

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_recipient_ingress_rejects_envelope_not_addressed_to_local_identity() {
    let (port, handle, _dir) = start_test_server().await;
    let client = reqwest::Client::new();
    let alice = NodeIdentity::generate();
    let carol = NodeIdentity::generate();
    let carol_identity = carol.identity_id();
    let (carol_key, _carol_private_key) =
        generate_identity_encryption_keypair(carol_identity.clone(), "carol-key".to_string(), 0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let envelope = EncryptedObjectEnvelope::encrypt(
        alice.public_key_bytes(),
        alice.identity_id(),
        b"not for bob",
        "application/octet-stream".to_string(),
        Some("application/vnd.spoke.reply+json".to_string()),
        vec![EncryptedObjectRecipient {
            identity: carol_identity,
            key: carol_key,
        }],
        now,
        |bytes| alice.sign(bytes),
    )
    .unwrap();

    let submit_resp = client
        .post(format!("{}/api/v1/ingress", base_url(port)))
        .json(&serde_json::json!({
            "receiver_id": "direct-local",
            "encrypted_object": envelope.to_bytes().unwrap(),
            "expires_at": now + 3600
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(submit_resp.status(), 400);
    let body: serde_json::Value = submit_resp.json().await.unwrap();
    assert_eq!(body["code"], "invalid_input");
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("not addressed to the local identity"));

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
