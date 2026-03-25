use std::time::Duration;

use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::{NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};
use tempfile::tempdir;

/// The critical M2 test:
/// Node A publishes content.
/// Node B fetches from A (auto-cached on B).
/// Node A goes offline.
/// Node C connects to B and fetches the same content from B's cache.
#[tokio::test]
async fn three_node_cache_relay() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let dir_c = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();
    let identity_b = NodeIdentity::generate();
    let identity_c = NodeIdentity::generate();

    let test_data = b"Content that will survive publisher going offline via caching!";

    // --- Step 1: Node A publishes content ---
    let store_a = ContentStore::open(dir_a.path(), CacheConfig::default()).unwrap();
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, NetworkConfig::test_config())
        .unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    let test_file = dir_a.path().join("test.txt");
    std::fs::write(&test_file, test_data).unwrap();
    let content_id = node_a.publish_file(&test_file).unwrap();

    // Wait for listen address
    let (mut node_a, addr_a) = {
        let handle = tokio::spawn(async move {
            loop {
                let event = node_a.next_event().await;
                node_a.handle_swarm_event(event);
                if !node_a.listeners().is_empty() {
                    return node_a;
                }
            }
        });
        let node_a = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
        let addr = node_a.listeners()[0].clone();
        (node_a, addr)
    };

    // --- Step 2: Node B fetches from A (auto-cached) ---
    let store_b = ContentStore::open(dir_b.path(), CacheConfig::default()).unwrap();
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, NetworkConfig::test_config())
        .unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let content_id_for_b = content_id.clone();
    let (tx_b, rx_b) = tokio::sync::oneshot::channel();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        // Wait for connection to A
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = tx_b.send(Err("Timed out connecting to A".to_string()));
                return node_b;
            }
            let event = tokio::time::timeout(Duration::from_millis(500), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
            if !node_b.connected_peers().is_empty() {
                break;
            }
        }

        // Fetch content
        let rx = node_b.request_content(&content_id_for_b).unwrap();
        let mut rx = rx;
        loop {
            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(resp)) => {
                            let _ = tx_b.send(Ok(resp));
                            return node_b;
                        }
                        Ok(Err(e)) => {
                            let _ = tx_b.send(Err(format!("{e}")));
                            return node_b;
                        }
                        Err(_) => {
                            let _ = tx_b.send(Err("channel closed".to_string()));
                            return node_b;
                        }
                    }
                }
                event = node_b.next_event() => {
                    node_b.handle_swarm_event(event);
                }
            }
        }
    });

    let response_b = tokio::time::timeout(Duration::from_secs(15), rx_b)
        .await
        .expect("timed out waiting for B's fetch")
        .expect("channel closed")
        .expect("B's fetch failed");

    assert_eq!(response_b.data, test_data, "B should get correct data from A");

    // Wait for node_b task to return
    let mut node_b = tokio::time::timeout(Duration::from_secs(5), node_b_handle)
        .await
        .expect("timed out waiting for B handle")
        .expect("B task failed");

    // Verify B has cached the content
    assert!(
        node_b.store().has_content(&content_id.to_string()),
        "B should have cached the content"
    );

    // --- Step 3: Shut down Node A ---
    node_a_handle.abort();

    // --- Step 4: Node C connects to B and fetches from B's cache ---

    // Get B's listen address
    let (mut node_b, addr_b) = {
        let handle = tokio::spawn(async move {
            loop {
                let event = node_b.next_event().await;
                node_b.handle_swarm_event(event);
                if !node_b.listeners().is_empty() {
                    return node_b;
                }
            }
        });
        let node_b = tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap();
        let addr = node_b.listeners()[0].clone();
        (node_b, addr)
    };

    let store_c = ContentStore::open(dir_c.path(), CacheConfig::default()).unwrap();
    let mut node_c = NetworkNode::new_tcp(identity_c, store_c, NetworkConfig::test_config())
        .unwrap();
    node_c.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_c.dial(addr_b).unwrap();

    let content_id_for_c = content_id.clone();
    let (tx_c, rx_c) = tokio::sync::oneshot::channel();

    let node_b_handle = tokio::spawn(async move {
        node_b.run_event_loop().await;
    });

    let node_c_handle = tokio::spawn(async move {
        // Wait for connection to B
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = tx_c.send(Err("Timed out connecting to B".to_string()));
                return;
            }
            let event = tokio::time::timeout(Duration::from_millis(500), node_c.next_event()).await;
            if let Ok(ev) = event {
                node_c.handle_swarm_event(ev);
            }
            if !node_c.connected_peers().is_empty() {
                break;
            }
        }

        // Fetch content -- this should come from B's cache since A is offline
        let rx = node_c.request_content(&content_id_for_c).unwrap();
        let mut rx = rx;
        loop {
            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(resp)) => {
                            let _ = tx_c.send(Ok(resp));
                            return;
                        }
                        Ok(Err(e)) => {
                            let _ = tx_c.send(Err(format!("{e}")));
                            return;
                        }
                        Err(_) => {
                            let _ = tx_c.send(Err("channel closed".to_string()));
                            return;
                        }
                    }
                }
                event = node_c.next_event() => {
                    node_c.handle_swarm_event(event);
                }
            }
        }
    });

    let response_c = tokio::time::timeout(Duration::from_secs(15), rx_c)
        .await
        .expect("timed out waiting for C's fetch")
        .expect("channel closed")
        .expect("C's fetch from B's cache failed");

    // Verify C got the correct content from B's cache
    assert_eq!(response_c.data, test_data, "C should get correct data from B's cache");
    assert!(content_id.verify(&response_c.data), "hash should verify");

    // Verify the original publisher's signature still verifies
    let sig_valid = verify_signature(&pubkey_a, &response_c.data, &response_c.signature).unwrap();
    assert!(sig_valid, "original publisher's signature should still verify");
    assert_eq!(response_c.publisher_key, pubkey_a.to_vec());

    node_b_handle.abort();
    node_c_handle.abort();
}
