use dweb_core::{ContentId, JoltAddress, PinRequest, UpdateAction, UpdateLogEntry};
use dweb_identity::NodeIdentity;
use dweb_network::{DaemonHandle, Multiaddr, NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};
use tokio::sync::mpsc;

/// Helper: spin up a daemon and HTTP server on a random port, return the port and handles.
async fn start_test_server() -> (u16, DaemonHandle, tempfile::TempDir) {
    start_test_server_with_node(None).await
}

async fn start_test_server_with_tcp_port(p2p_port: u16) -> (u16, DaemonHandle, tempfile::TempDir) {
    start_test_server_with_node(Some(p2p_port)).await
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
    let (port, _server_handle) = dweb_server::server::start_server_with_addr(handle.clone(), 0)
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

fn free_tcp_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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
    assert!(body["home_relay"].is_null());

    handle.shutdown().await.ok();
}

#[tokio::test]
async fn test_status_endpoint_reports_home_relay_config() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
    let mut config = NetworkConfig::test_config();
    config.home_relay = Some(dweb_network::HomeRelayConfig {
        peer_id: "12D3HomeRelay".to_string(),
        multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3HomeRelay".to_string(),
        capability: dweb_network::HomeRelayCapability::Pinning,
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

    handle.shutdown().await.ok();
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
    assert!(body["error"].as_str().unwrap().contains("jolt:update-log"));

    handle.shutdown().await.ok();
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
