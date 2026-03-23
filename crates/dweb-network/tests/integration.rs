use std::time::Duration;

use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::{NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};
use tempfile::tempdir;

fn make_store(dir: &std::path::Path) -> ContentStore {
    ContentStore::open(dir, CacheConfig::default()).unwrap()
}

#[tokio::test]
async fn two_nodes_publish_and_fetch() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();
    let identity_b = NodeIdentity::generate();

    // Create Node A and start listening
    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new(identity_a, store_a, NetworkConfig::test_config()).await.unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    // Publish test content on Node A
    let test_data = b"Hello from dweb node A! This is a test of peer-to-peer content exchange.";
    let test_file = dir_a.path().join("test.txt");
    std::fs::write(&test_file, test_data).unwrap();
    let content_id = node_a.publish_file(&test_file).unwrap();

    // Get Node A's listen address
    let (mut node_a, addr_a) = {
        let handle = tokio::spawn(async move {
            loop {
                let event = node_a.next_event().await;
                node_a.handle_swarm_event(event);
                let addrs = node_a.listeners();
                if !addrs.is_empty() {
                    return node_a;
                }
            }
        });
        let node_a = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("timed out")
            .expect("task failed");
        let addr = node_a.listeners()[0].clone();
        (node_a, addr)
    };

    // Create Node B and dial Node A
    let store_b = make_store(dir_b.path());
    let mut node_b = NetworkNode::new(identity_b, store_b, NetworkConfig::test_config()).await.unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let content_id_clone = content_id.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        // Wait for connection
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = tx_result.send(Err("Timed out waiting for connection".to_string()));
                return;
            }
            let event = tokio::time::timeout(Duration::from_secs(1), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
            if !node_b.connected_peers().is_empty() {
                break;
            }
        }

        // Request content
        let rx = match node_b.request_content(&content_id_clone) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = tx_result.send(Err(format!("Failed to request: {e}")));
                return;
            }
        };

        let mut rx = rx;
        loop {
            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(response)) => {
                            // Check that content was auto-cached
                            let cached = node_b.store().has_content(&content_id_clone.to_string());
                            let _ = tx_result.send(Ok((response, cached)));
                            return;
                        }
                        Ok(Err(e)) => {
                            let _ = tx_result.send(Err(format!("Fetch error: {e}")));
                            return;
                        }
                        Err(_) => {
                            let _ = tx_result.send(Err("Channel closed".to_string()));
                            return;
                        }
                    }
                }
                event = node_b.next_event() => {
                    node_b.handle_swarm_event(event);
                }
            }
        }
    });

    let (response, was_cached) = tokio::time::timeout(Duration::from_secs(15), rx_result)
        .await
        .expect("timed out")
        .expect("channel closed")
        .expect("fetch failed");

    assert_eq!(response.data, test_data);
    assert!(content_id.verify(&response.data));

    let sig_valid = verify_signature(&pubkey_a, &response.data, &response.signature).unwrap();
    assert!(sig_valid);
    assert_eq!(response.publisher_key, pubkey_a.to_vec());

    // Verify auto-caching happened
    assert!(was_cached, "fetched content should be auto-cached on Node B");

    node_a_handle.abort();
    node_b_handle.abort();
}
