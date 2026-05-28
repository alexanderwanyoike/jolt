use dweb_identity::NodeIdentity;
use dweb_network::{DaemonHandle, NetworkConfig, NetworkNode};
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

    // Use short fetch timeout for tests
    node.set_fetch_timeout(std::time::Duration::from_secs(2));

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
    assert!(body.contains("/api/v1/peers/connect"));

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
    assert_eq!(connected.direct_peers, 1);

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
