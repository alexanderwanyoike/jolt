use std::time::Duration;

use dweb_core::ContentId;
use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::NetworkNode;
use tempfile::tempdir;

#[tokio::test]
async fn two_nodes_publish_and_fetch() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // Create two separate identities and content stores
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();

    let identity_b = NodeIdentity::generate();

    // Create Node A and start listening
    let mut node_a = NetworkNode::new(identity_a, dir_a.path().join("store"))
        .await
        .unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    // Publish test content on Node A
    let test_data = b"Hello from dweb node A! This is a test of peer-to-peer content exchange.";
    let test_file = dir_a.path().join("test.txt");
    std::fs::write(&test_file, test_data).unwrap();

    let content_id = node_a.publish_file(&test_file).unwrap();

    // Run Node A's event loop in background to process connection
    let (mut node_a, addr_a) = {
        // We need to pump the event loop briefly to get the actual listen address
        let handle = tokio::spawn(async move {
            // Process events until we have a listen address
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
            .expect("timed out waiting for listen address")
            .expect("node A task failed");

        let addr = node_a.listeners()[0].clone();
        (node_a, addr)
    };

    // Create Node B
    let mut node_b = NetworkNode::new(identity_b, dir_b.path().join("store"))
        .await
        .unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    // Node B dials Node A directly (skip mDNS for test reliability)
    node_b.dial(addr_a).unwrap();

    // Run both event loops in background tasks
    let content_id_clone = content_id.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        // First, pump events until we're connected to node A
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > deadline {
                let _ = tx_result.send(Err("Timed out waiting for connection".to_string()));
                return;
            }

            let event = tokio::time::timeout(Duration::from_secs(1), node_b.next_event()).await;
            match event {
                Ok(ev) => node_b.handle_swarm_event(ev),
                Err(_) => {}
            }

            if !node_b.connected_peers().is_empty() {
                break;
            }
        }

        // Now request the content
        let rx = match node_b.request_content(&content_id_clone) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = tx_result.send(Err(format!("Failed to request content: {e}")));
                return;
            }
        };

        // Keep pumping events while waiting for the response
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

    // Wait for the result with a timeout
    let response = tokio::time::timeout(Duration::from_secs(15), rx_result)
        .await
        .expect("timed out waiting for fetch result")
        .expect("result channel closed")
        .expect("fetch failed");

    // Verify the content
    assert_eq!(response.data, test_data, "fetched data should match original");

    // Verify content ID matches
    assert!(
        content_id.verify(&response.data),
        "ContentId should verify against fetched data"
    );

    // Verify the signature
    let sig_valid =
        verify_signature(&pubkey_a, &response.data, &response.signature).unwrap();
    assert!(sig_valid, "signature should verify against publisher's key");

    assert_eq!(
        response.publisher_key,
        pubkey_a.to_vec(),
        "publisher key should match node A"
    );

    // Clean up
    node_a_handle.abort();
    node_b_handle.abort();
}
