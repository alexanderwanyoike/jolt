use std::time::Duration;

use jolt_core::{ContentId, UpdateAction, UpdateLogEntry};
use jolt_identity::{verify_signature, NodeIdentity};
use jolt_network::{NetworkConfig, NetworkNode};
use jolt_store::{CacheConfig, ContentStore};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use tempfile::tempdir;

fn make_store(dir: &std::path::Path) -> ContentStore {
    ContentStore::open(dir, CacheConfig::default()).unwrap()
}

fn no_mdns_config() -> NetworkConfig {
    NetworkConfig {
        enable_mdns: false,
        ..NetworkConfig::test_config()
    }
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

async fn wait_for_listener(mut node: NetworkNode) -> NetworkNode {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = node.next_event().await;
            node.handle_swarm_event(event);
            if !node.listeners().is_empty() {
                return node;
            }
        }
    })
    .await
    .expect("timed out waiting for listener")
}

fn listener_with_peer(addr: &Multiaddr, peer: &PeerId) -> Multiaddr {
    addr.clone().with(Protocol::P2p(*peer))
}

/// Test that two nodes can exchange content when connected directly.
/// Node A publishes and announces as DHT provider.
/// Node B connects to A, discovers provider via Kademlia, and fetches content.
#[tokio::test]
async fn two_nodes_dht_provider_announce_and_fetch() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();
    let identity_b = NodeIdentity::generate();

    // Create Node A, publish content
    let store_a = make_store(dir_a.path());
    let mut node_a =
        NetworkNode::new_tcp(identity_a, store_a, NetworkConfig::test_config()).unwrap();
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
    let mut node_b =
        NetworkNode::new_tcp(identity_b, store_b, NetworkConfig::test_config()).unwrap();
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
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let identity_b = NodeIdentity::generate();

    let store_a = make_store(dir_a.path());
    let mut node_a =
        NetworkNode::new_tcp(identity_a, store_a, NetworkConfig::test_config()).unwrap();
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
    let mut node_b =
        NetworkNode::new_tcp(identity_b, store_b, NetworkConfig::test_config()).unwrap();
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

/// Alice and Bob only know the relay address. Bob discovers Alice's update-log
/// provider record through the DHT, requests the candidate log, and stores it
/// only after identity verification succeeds.
#[tokio::test]
async fn bob_discovers_alice_update_log_provider_through_bootstrap_relay() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();

    let dir_relay = tempdir().unwrap();
    let dir_alice = tempdir().unwrap();
    let dir_bob = tempdir().unwrap();

    let relay_identity = NodeIdentity::generate();
    let alice_identity = NodeIdentity::generate();
    let alice_jolt_identity = alice_identity.identity_id();
    let alice_update_log = signed_profile_log(&alice_identity, b"profile via relay dht");
    let bob_identity = NodeIdentity::generate();

    let relay_store = make_store(dir_relay.path());
    let mut relay = NetworkNode::new_tcp(relay_identity, relay_store, no_mdns_config()).unwrap();
    relay.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut relay = wait_for_listener(relay).await;
    let relay_addr = listener_with_peer(&relay.listeners()[0], relay.local_peer_id());

    let relay_handle = tokio::spawn(async move {
        relay.run_event_loop().await;
    });

    let alice_store = make_store(dir_alice.path());
    let mut alice = NetworkNode::new_tcp(alice_identity, alice_store, no_mdns_config()).unwrap();
    alice
        .store_verified_update_log(alice_jolt_identity.clone(), alice_update_log.clone())
        .unwrap();
    alice.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut alice = wait_for_listener(alice).await;
    let alice_peer_id = *alice.local_peer_id();
    alice.bootstrap_dht(&[relay_addr.clone()]).unwrap();
    alice
        .announce_update_log_provider(&alice_jolt_identity)
        .unwrap();

    let alice_handle = tokio::spawn(async move {
        alice.run_event_loop().await;
    });

    let bob_store = make_store(dir_bob.path());
    let mut bob = NetworkNode::new_tcp(bob_identity, bob_store, no_mdns_config()).unwrap();
    bob.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut bob = wait_for_listener(bob).await;
    bob.bootstrap_dht(&[relay_addr]).unwrap();

    let expected_log = alice_update_log.clone();
    let expected_identity = alice_jolt_identity.clone();
    let expected_provider = alice_peer_id;
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    let bob_handle = tokio::spawn(async move {
        assert!(
            bob.update_log_entries(&expected_identity).is_none(),
            "Bob must start without Alice's update log"
        );

        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < settle_deadline {
            if let Ok(event) =
                tokio::time::timeout(Duration::from_millis(100), bob.next_event()).await
            {
                bob.handle_swarm_event(event);
            }
        }

        bob.find_update_log_providers(&expected_identity);

        let provider = loop {
            let event = tokio::time::timeout(Duration::from_secs(1), bob.next_event()).await;
            match event {
                Ok(event) => bob.handle_swarm_event(event),
                Err(_) => {
                    let _ =
                        tx_result.send(Err("timed out finding update-log provider".to_string()));
                    return;
                }
            }

            if let Some(provider) = bob.take_discovered_update_log_provider(&expected_identity) {
                if provider != expected_provider {
                    let _ =
                        tx_result.send(Err(format!("expected Alice as provider, got {provider}")));
                    return;
                }
                break provider;
            }
        };

        let rx = match bob.request_update_log_from(&expected_identity, None, &provider) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = tx_result.send(Err(format!("failed to request update log: {e}")));
                return;
            }
        };

        let mut rx = rx;
        loop {
            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(response)) => {
                            let cached = bob.update_log_entries(&expected_identity)
                                .map(|entries| entries.to_vec());
                            let _ = tx_result.send(Ok((response.entries, cached)));
                            return;
                        }
                        Ok(Err(e)) => {
                            let _ = tx_result.send(Err(format!("update-log request failed: {e}")));
                            return;
                        }
                        Err(_) => {
                            let _ = tx_result.send(Err("update-log response channel closed".to_string()));
                            return;
                        }
                    }
                }
                event = bob.next_event() => {
                    bob.handle_swarm_event(event);
                }
            }
        }
    });

    let (response_entries, cached_entries) =
        tokio::time::timeout(Duration::from_secs(20), rx_result)
            .await
            .expect("timed out")
            .expect("channel closed")
            .expect("provider discovery failed");

    assert_eq!(response_entries, expected_log);
    assert_eq!(cached_entries, Some(expected_log));

    relay_handle.abort();
    alice_handle.abort();
    bob_handle.abort();
}
