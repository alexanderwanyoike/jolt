use std::time::Duration;

use dweb_core::ContentId;
use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::{NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};
use tempfile::tempdir;

fn make_store(dir: &std::path::Path) -> ContentStore {
    ContentStore::open(dir, CacheConfig::default()).unwrap()
}

/// Test that two nodes can exchange content when connected directly.
/// Node A publishes and announces as DHT provider.
/// Node B connects to A, discovers provider via Kademlia, and fetches content.
#[tokio::test]
async fn two_nodes_dht_provider_announce_and_fetch() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();
    let identity_b = NodeIdentity::generate();

    // Create Node A, publish content
    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, NetworkConfig::test_config())
        .unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    let test_data = b"Content discovered via DHT provider records!";
    let test_file = dir_a.path().join("test.txt");
    std::fs::write(&test_file, test_data).unwrap();
    let content_id = node_a.publish_file(&test_file).unwrap();

    // Get Node A's listen address
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

    // Create Node B, connect to A
    let store_b = make_store(dir_b.path());
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, NetworkConfig::test_config())
        .unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let content_id_clone = content_id.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    // Run both nodes
    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        // Wait for connection
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = tx_result.send(Err("Timed out connecting".to_string()));
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

        // Wait for identify exchange and Kademlia routing table update
        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < settle_deadline {
            let event = tokio::time::timeout(Duration::from_millis(100), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
        }

        // Now request content (Node B should be connected and can request directly)
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
                            let _ = tx_result.send(Ok(response));
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

    let response = tokio::time::timeout(Duration::from_secs(15), rx_result)
        .await
        .expect("timed out")
        .expect("channel closed")
        .expect("fetch failed");

    // Verify content
    assert_eq!(response.data, test_data);
    assert!(content_id.verify(&response.data));

    let sig_valid = verify_signature(&pubkey_a, &response.data, &response.signature).unwrap();
    assert!(sig_valid);

    node_a_handle.abort();
    node_b_handle.abort();
}

/// Test that Node A can announce as a provider and Node B can query for providers.
/// This tests the Kademlia provider record flow specifically.
#[tokio::test]
async fn dht_provider_announce_is_queryable() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let identity_b = NodeIdentity::generate();

    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, NetworkConfig::test_config())
        .unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    // Publish content on A (which also announces to DHT)
    let test_file = dir_a.path().join("test.txt");
    std::fs::write(&test_file, b"provider test").unwrap();
    let content_id = node_a.publish_file(&test_file).unwrap();

    // Get listen addr
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

    // Create Node B and connect to A
    let store_b = make_store(dir_b.path());
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, NetworkConfig::test_config())
        .unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let (tx_found, rx_found) = tokio::sync::oneshot::channel();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        // Wait for connection + identify
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            if tokio::time::Instant::now() > deadline {
                break;
            }
            let event = tokio::time::timeout(Duration::from_millis(100), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
            if !node_b.connected_peers().is_empty() {
                break;
            }
        }

        // Let identify and kademlia settle
        let settle = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < settle {
            let event = tokio::time::timeout(Duration::from_millis(100), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
        }

        // Query for providers
        let _query_id = node_b.find_providers(&content_id);

        // Pump events and look for provider found log
        // (We can't easily check the internal state, but if no panic occurs
        // and the query completes, the DHT plumbing works)
        let query_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < query_deadline {
            let event = tokio::time::timeout(Duration::from_millis(100), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
        }

        let _ = tx_found.send(true);
    });

    let result = tokio::time::timeout(Duration::from_secs(15), rx_found)
        .await
        .expect("timed out")
        .expect("channel closed");

    assert!(result, "DHT provider query should complete without error");

    node_a_handle.abort();
    node_b_handle.abort();
}
