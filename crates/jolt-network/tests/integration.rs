use std::{
    collections::BTreeMap,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jolt_core::{
    ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
    DeviceWriterOperation, DeviceWriterPathMode, IdentityHeadHint, JoltAddress, RelayRecord,
    RelayRecordCapability, UpdateAction, UpdateLogEntry,
};
use jolt_identity::{verify_signature, NodeIdentity};
use jolt_network::{
    DaemonCommand, DaemonHandle, LocalRecordHead, LocalRecordState, NetworkConfig, NetworkNode,
};
use jolt_store::{CacheConfig, ContentStore, PersistedDeviceWriterLog};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId};
use tempfile::tempdir;

static INTEGRATION_TEST_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn integration_test_lock() -> &'static tokio::sync::Mutex<()> {
    INTEGRATION_TEST_LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn make_store(dir: &std::path::Path) -> ContentStore {
    ContentStore::open(dir, CacheConfig::default()).unwrap()
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_secs()
}

fn relay_record(identity: &NodeIdentity, port: u16) -> RelayRecord {
    RelayRecord::new(
        identity.public_key_bytes(),
        identity.peer_id().to_string(),
        vec![format!("/ip4/127.0.0.1/tcp/{port}")],
        vec![
            RelayRecordCapability::Bootstrap,
            RelayRecordCapability::Discovery,
        ],
        100,
        4_102_444_800,
        |bytes| identity.sign(bytes),
    )
    .unwrap()
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

fn authorize_test_devices(
    owner: &NodeIdentity,
    devices: &[&NodeIdentity],
) -> Vec<DeviceAuthorizationRecord> {
    let identity = owner.identity_id();
    let mut records: Vec<DeviceAuthorizationRecord> = Vec::new();

    for (index, device) in devices.iter().enumerate() {
        let device_id = format!("dev_{}", device.identity_id());
        let operation = DeviceAuthorizationOperation::authorize_device(
            device_id,
            device.public_key_bytes(),
            vec!["identity:write".to_string()],
            Some(format!("Test installation {}", index + 1)),
            100 + index as u64,
        );
        let record = match records.last() {
            Some(previous) => previous
                .append(operation, 100 + index as u64, |bytes| owner.sign(bytes))
                .unwrap(),
            None => DeviceAuthorizationRecord::genesis(
                owner.public_key_bytes(),
                identity.clone(),
                operation,
                100,
                |bytes| owner.sign(bytes),
            )
            .unwrap(),
        };
        records.push(record);
    }

    records
}

fn provision_test_installation(
    store: &ContentStore,
    owner: &NodeIdentity,
    device: &NodeIdentity,
    authority_records: &[DeviceAuthorizationRecord],
) {
    provision_test_installation_with_history(store, owner, device, authority_records, Vec::new());
}

fn provision_test_installation_with_history(
    store: &ContentStore,
    owner: &NodeIdentity,
    device: &NodeIdentity,
    authority_records: &[DeviceAuthorizationRecord],
    other_device_logs: Vec<Vec<DeviceWriterLogEntry>>,
) {
    store
        .save_local_device_signing_key(&device.signing_key_bytes())
        .unwrap();
    store
        .save_device_writer_log(
            &owner.identity_id(),
            &PersistedDeviceWriterLog {
                authority_records: authority_records.to_vec(),
                device_log: Vec::new(),
                other_device_logs,
                record_mutations: BTreeMap::new(),
            },
        )
        .unwrap();
}

async fn inspect_restarted_local_record(mut node: NetworkNode, path: &str) -> LocalRecordState {
    let (tx, rx) = tokio::sync::mpsc::channel::<DaemonCommand>(4);
    let handle = DaemonHandle::new(tx);
    let daemon = tokio::spawn(async move { node.run_daemon_loop(rx).await });
    let state = handle.inspect_local_record(path.to_string()).await.unwrap();
    handle.shutdown().await.unwrap();
    daemon.await.unwrap();
    state
}

fn signed_identity_head_hint(
    identity: &NodeIdentity,
    provider_peer: &PeerId,
    provider_addrs: Vec<String>,
    entries: &[UpdateLogEntry],
    now: u64,
) -> IdentityHeadHint {
    let latest = entries
        .last()
        .expect("identity head hint requires at least one update-log entry");
    IdentityHeadHint::new(
        identity.public_key_bytes(),
        identity.identity_id(),
        provider_peer.to_string(),
        provider_addrs,
        None,
        latest.body.sequence,
        latest.entry_hash(),
        now,
        now + 60,
        |bytes| identity.sign(bytes),
    )
    .unwrap()
}

#[tokio::test]
async fn two_nodes_publish_and_fetch() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let pubkey_a = identity_a.public_key_bytes();
    let identity_b = NodeIdentity::generate();

    // Create Node A and start listening
    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, no_mdns_config()).unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    // Publish test content on Node A
    let test_data = b"Hello from jolt node A! This is a test of peer-to-peer content exchange.";
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
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, no_mdns_config()).unwrap();
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
    assert!(
        was_cached,
        "fetched content should be auto-cached on Node B"
    );

    node_a_handle.abort();
    node_b_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_owner_installations_sync_device_writer_records_over_local_tcp() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;
    let first_dir = tempdir().unwrap();
    let second_dir = tempdir().unwrap();
    let owner = NodeIdentity::generate();
    let owner_signing_key = owner.signing_key_bytes();
    let first_device = NodeIdentity::generate();
    let second_device = NodeIdentity::generate();
    let authority_records = authorize_test_devices(&owner, &[&first_device, &second_device]);

    let first_store = make_store(first_dir.path());
    provision_test_installation(&first_store, &owner, &first_device, &authority_records);
    let second_store = make_store(second_dir.path());
    provision_test_installation(&second_store, &owner, &second_device, &authority_records);

    let mut first = NetworkNode::new_tcp(owner, first_store, no_mdns_config()).unwrap();
    let mut second = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        second_store,
        no_mdns_config(),
    )
    .unwrap();
    let first_file = first_dir.path().join("first.json");
    std::fs::write(&first_file, br#"{"text":"first"}"#).unwrap();
    first
        .publish_file_appending_path(&first_file, "/card130/posts/first")
        .unwrap();
    let second_file = second_dir.path().join("second.json");
    std::fs::write(&second_file, br#"{"text":"second"}"#).unwrap();
    second
        .publish_file_appending_path(&second_file, "/card130/posts/second")
        .unwrap();

    first.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    first = wait_for_listener(first).await;
    let first_addr = listener_with_peer(&first.listeners()[0], &first.local_peer_id());
    second.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    second = wait_for_listener(second).await;
    let second_addr = listener_with_peer(&second.listeners()[0], &second.local_peer_id());
    second.dial(first_addr.clone()).unwrap();
    let (first_tx, first_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let (second_tx, second_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let first_handle = DaemonHandle::new(first_tx);
    let second_handle = DaemonHandle::new(second_tx);
    let first_daemon = tokio::spawn(async move { first.run_daemon_loop(first_rx).await });
    let second_daemon = tokio::spawn(async move { second.run_daemon_loop(second_rx).await });
    let identity = NodeIdentity::from_signing_key_bytes(&owner_signing_key)
        .unwrap()
        .identity_id();

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if !first_handle.peers().await.unwrap().is_empty()
                && !second_handle.peers().await.unwrap().is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("same-owner installations did not connect over local TCP");
    let (first_count, second_count) = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let first_count = first_handle
                .enumerate_append_records(identity.clone(), "/card130/posts/".to_string())
                .await
                .unwrap()
                .len();
            let second_count = second_handle
                .enumerate_append_records(identity.clone(), "/card130/posts/".to_string())
                .await
                .unwrap()
                .len();
            if first_count == 2 || second_count == 2 {
                break (first_count, second_count);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("the first local synchronization direction did not complete");
    if first_count == 1 {
        first_handle
            .connect_peer(second_addr.to_string())
            .await
            .unwrap();
    } else if second_count == 1 {
        second_handle
            .connect_peer(first_addr.to_string())
            .await
            .unwrap();
    }

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let first_records = first_handle
                .enumerate_append_records(identity.clone(), "/card130/posts/".to_string())
                .await
                .unwrap();
            let second_records = second_handle
                .enumerate_append_records(identity.clone(), "/card130/posts/".to_string())
                .await
                .unwrap();
            if first_records.len() == 2 && second_records.len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("same-owner installations did not synchronize over local TCP");

    first_handle.shutdown().await.unwrap();
    second_handle.shutdown().await.unwrap();
    first_daemon.await.unwrap();
    second_daemon.await.unwrap();

    let first_restarted = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        make_store(first_dir.path()),
        no_mdns_config(),
    )
    .unwrap();
    let second_restarted = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        make_store(second_dir.path()),
        no_mdns_config(),
    )
    .unwrap();
    assert_eq!(
        first_restarted
            .enumerate_append_records(&identity, "/card130/posts/")
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        second_restarted
            .enumerate_append_records(&identity, "/card130/posts/")
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_owner_installations_converge_a_resolved_singleton_conflict_over_local_tcp() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;
    let first_dir = tempdir().unwrap();
    let second_dir = tempdir().unwrap();
    let owner = NodeIdentity::generate();
    let owner_signing_key = owner.signing_key_bytes();
    let first_device = NodeIdentity::generate();
    let second_device = NodeIdentity::generate();
    let seed_device = NodeIdentity::generate();
    let authority_records =
        authorize_test_devices(&owner, &[&first_device, &second_device, &seed_device]);
    let identity = owner.identity_id();
    let path = "/card130/profile";
    let base_bytes = br#"{"version":1,"value":{"name":"Original"}}"#;
    let base_content = ContentId::from_bytes(base_bytes);
    let base = DeviceWriterLogEntry::genesis(
        identity.clone(),
        format!("dev_{}", seed_device.identity_id()),
        DeviceWriterOperation::set_path(
            path,
            base_content.clone(),
            DeviceWriterPathMode::Singleton,
        ),
        200,
        |bytes| seed_device.sign(bytes),
    )
    .unwrap();
    let base_revision = base.entry_hash().to_hex();

    let first_store = make_store(first_dir.path());
    provision_test_installation_with_history(
        &first_store,
        &owner,
        &first_device,
        &authority_records,
        vec![vec![base.clone()]],
    );
    let second_store = make_store(second_dir.path());
    provision_test_installation_with_history(
        &second_store,
        &owner,
        &second_device,
        &authority_records,
        vec![vec![base]],
    );

    let mut first = NetworkNode::new_tcp(owner, first_store, no_mdns_config()).unwrap();
    let mut second = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        second_store,
        no_mdns_config(),
    )
    .unwrap();
    let first_base_file = first_dir.path().join("base.json");
    let second_base_file = second_dir.path().join("base.json");
    std::fs::write(&first_base_file, base_bytes).unwrap();
    std::fs::write(&second_base_file, base_bytes).unwrap();
    assert_eq!(first.publish_file(&first_base_file).unwrap(), base_content);
    assert_eq!(
        second.publish_file(&second_base_file).unwrap(),
        base_content
    );

    let (first_tx, first_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let (second_tx, second_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let first_handle = DaemonHandle::new(first_tx);
    let second_handle = DaemonHandle::new(second_tx);
    let first_daemon = tokio::spawn(async move { first.run_daemon_loop(first_rx).await });
    let second_daemon = tokio::spawn(async move { second.run_daemon_loop(second_rx).await });
    let first_update = first_handle
        .update_local_record(
            path.to_string(),
            br#"{"version":1,"value":{"name":"First"}}"#.to_vec(),
            base_revision.clone(),
            vec![base_revision.clone()],
            "mut_first_offline".to_string(),
        )
        .await
        .unwrap();
    let second_update = second_handle
        .update_local_record(
            path.to_string(),
            br#"{"version":1,"value":{"name":"Second"}}"#.to_vec(),
            base_revision.clone(),
            vec![base_revision.clone()],
            "mut_second_offline".to_string(),
        )
        .await
        .unwrap();
    first_handle.shutdown().await.unwrap();
    second_handle.shutdown().await.unwrap();
    first_daemon.await.unwrap();
    second_daemon.await.unwrap();

    let mut first = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        make_store(first_dir.path()),
        no_mdns_config(),
    )
    .unwrap();
    let mut second = NetworkNode::new_tcp(
        NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
        make_store(second_dir.path()),
        no_mdns_config(),
    )
    .unwrap();
    first.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    first = wait_for_listener(first).await;
    second.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    second = wait_for_listener(second).await;
    let second_addr = listener_with_peer(&second.listeners()[0], &second.local_peer_id());
    first.dial(second_addr).unwrap();
    let (first_tx, first_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let (second_tx, second_rx) = tokio::sync::mpsc::channel::<DaemonCommand>(16);
    let first_handle = DaemonHandle::new(first_tx);
    let second_handle = DaemonHandle::new(second_tx);
    let first_daemon = tokio::spawn(async move { first.run_daemon_loop(first_rx).await });
    let second_daemon = tokio::spawn(async move { second.run_daemon_loop(second_rx).await });

    let conflict = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let first_state = first_handle
                .inspect_local_record(path.to_string())
                .await
                .unwrap();
            let second_state = second_handle
                .inspect_local_record(path.to_string())
                .await
                .unwrap();
            if matches!(first_state, LocalRecordState::Conflicted { .. })
                && first_state == second_state
            {
                break first_state;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("same-owner installations did not expose the same conflict");
    let (alternatives, base) = match conflict {
        LocalRecordState::Conflicted {
            alternatives, base, ..
        } => (alternatives, base),
        state => panic!("expected conflicted state, got {state:?}"),
    };
    let observed_revisions: Vec<String> = alternatives
        .iter()
        .map(|alternative| match alternative {
            LocalRecordHead::Deleted { revision } => revision.clone(),
            LocalRecordHead::Present(record) => record.revision.clone(),
        })
        .collect();
    assert_eq!(observed_revisions.len(), 2);
    assert!(observed_revisions.contains(&first_update.revision));
    assert!(observed_revisions.contains(&second_update.revision));
    assert_eq!(
        base,
        Some(LocalRecordHead::Present(jolt_network::LocalRecordInfo {
            path: path.to_string(),
            content_id: base_content.to_string(),
            revision: base_revision,
        }))
    );

    let resolved_bytes = br#"{"version":1,"value":{"name":"Resolved"}}"#.to_vec();
    let resolved = first_handle
        .update_local_record(
            path.to_string(),
            resolved_bytes,
            observed_revisions.last().unwrap().clone(),
            observed_revisions,
            "mut_resolve_both_heads".to_string(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let expected = LocalRecordState::Present(jolt_network::LocalRecordInfo {
                path: path.to_string(),
                content_id: resolved.content_id.clone(),
                revision: resolved.revision.clone(),
            });
            if first_handle
                .inspect_local_record(path.to_string())
                .await
                .unwrap()
                == expected
                && second_handle
                    .inspect_local_record(path.to_string())
                    .await
                    .unwrap()
                    == expected
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("resolved singleton did not converge while both installations were connected");

    first_handle.shutdown().await.unwrap();
    second_handle.shutdown().await.unwrap();
    first_daemon.await.unwrap();
    second_daemon.await.unwrap();
    for directory in [first_dir.path(), second_dir.path()] {
        let restarted = NetworkNode::new_tcp(
            NodeIdentity::from_signing_key_bytes(&owner_signing_key).unwrap(),
            make_store(directory),
            no_mdns_config(),
        )
        .unwrap();
        assert_eq!(
            inspect_restarted_local_record(restarted, path).await,
            LocalRecordState::Present(jolt_network::LocalRecordInfo {
                path: path.to_string(),
                content_id: resolved.content_id.clone(),
                revision: resolved.revision.clone(),
            })
        );
    }
}

#[tokio::test]
async fn two_nodes_request_and_cache_verified_update_log() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let jolt_identity = identity_a.identity_id();
    let update_log = signed_profile_log(&identity_a, b"profile");
    let identity_b = NodeIdentity::generate();

    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, no_mdns_config()).unwrap();
    node_a
        .store_verified_update_log(jolt_identity.clone(), update_log.clone())
        .unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

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
            .expect("timed out")
            .expect("task failed");
        let addr = node_a.listeners()[0].clone();
        (node_a, addr)
    };

    let store_b = make_store(dir_b.path());
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, no_mdns_config()).unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let identity_clone = jolt_identity.clone();
    let address = JoltAddress::new(jolt_identity.clone(), "/profile").unwrap();
    let expected_log = update_log.clone();
    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    let node_b_handle = tokio::spawn(async move {
        let peer = loop {
            let event = tokio::time::timeout(Duration::from_secs(1), node_b.next_event()).await;
            if let Ok(ev) = event {
                node_b.handle_swarm_event(ev);
            }
            if let Some(peer) = node_b.connected_peers().first().cloned() {
                break peer;
            }
        };

        let rx = match node_b.request_jolt_address_from_peer(&address, None, &peer) {
            Ok(rx) => rx,
            Err(e) => {
                let _ = tx_result.send(Err(format!("Failed to resolve address: {e}")));
                return;
            }
        };

        let mut rx = rx;
        loop {
            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(target)) => {
                            let cached = node_b.update_log_entries(&identity_clone)
                                .map(|entries| entries.to_vec());
                            let _ = tx_result.send(Ok((target.content_id, cached)));
                            return;
                        }
                        Ok(Err(e)) => {
                            let _ = tx_result.send(Err(format!("Resolution error: {e}")));
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

    let (resolved_content_id, cached_entries) =
        tokio::time::timeout(Duration::from_secs(15), rx_result)
            .await
            .expect("timed out")
            .expect("channel closed")
            .expect("address resolution failed");

    assert_eq!(cached_entries, Some(expected_log));
    assert_eq!(resolved_content_id, ContentId::from_bytes(b"profile"));

    node_a_handle.abort();
    node_b_handle.abort();
}

#[tokio::test]
async fn connected_peer_address_is_cached_for_later_bootstrap() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let identity_a = NodeIdentity::generate();
    let identity_b = NodeIdentity::generate();

    let store_a = make_store(dir_a.path());
    let mut node_a = NetworkNode::new_tcp(identity_a, store_a, no_mdns_config()).unwrap();
    node_a.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();

    let (mut node_a, addr_a, peer_a) = {
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
            .expect("timed out")
            .expect("task failed");
        let addr = node_a.listeners()[0].clone();
        let peer = *node_a.local_peer_id();
        (node_a, addr, peer)
    };

    let store_b = make_store(dir_b.path());
    let mut node_b = NetworkNode::new_tcp(identity_b, store_b, no_mdns_config()).unwrap();
    node_b.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_b.dial(addr_a).unwrap();

    let node_a_handle = tokio::spawn(async move {
        node_a.run_event_loop().await;
    });

    tokio::time::timeout(Duration::from_secs(10), async move {
        loop {
            let event = node_b.next_event().await;
            node_b.handle_swarm_event(event);
            if !node_b.connected_peers().is_empty() {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for connection");

    node_a_handle.abort();

    let store_b = make_store(dir_b.path());
    let hints = store_b.load_discovered_peer_hints().unwrap();

    assert_eq!(hints.len(), 1);
    assert!(hints[0].multiaddr.ends_with(&format!("/p2p/{peer_a}")));
}

#[tokio::test]
async fn connected_relay_exchange_persists_learned_relays() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r1 = tempdir().unwrap();
    let dir_tim = tempdir().unwrap();

    let identity_r1 = NodeIdentity::generate();
    let identity_r2 = NodeIdentity::generate();
    let identity_r3 = NodeIdentity::generate();
    let identity_tim = NodeIdentity::generate();

    let store_r1 = make_store(dir_r1.path());
    store_r1
        .record_relay_record(relay_record(&identity_r2, 4002), 120)
        .unwrap();
    store_r1
        .record_relay_record(relay_record(&identity_r3, 4003), 130)
        .unwrap();

    let mut node_r1 = NetworkNode::new_tcp(identity_r1, store_r1, relay_config()).unwrap();
    node_r1.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r1 = wait_for_listener(node_r1).await;
    let r1_addr = listener_with_peer(&node_r1.listeners()[0], node_r1.local_peer_id());

    let r1_handle = tokio::spawn(async move {
        node_r1.run_event_loop().await;
    });

    let mut tim_config = no_mdns_config();
    tim_config.effective_bootstrap_relays = vec![r1_addr.to_string()];
    let store_tim = make_store(dir_tim.path());
    let mut node_tim = NetworkNode::new_tcp(identity_tim, store_tim, tim_config).unwrap();
    node_tim.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    node_tim
        .connect_peer_multiaddr(&r1_addr.to_string())
        .unwrap();

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = node_tim.next_event().await;
            node_tim.handle_swarm_event(event);
            if node_tim.store().known_relay_count(200).unwrap() >= 2 {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for relay exchange");

    let records = node_tim.store().load_relay_records(200).unwrap();
    let relay_ids: Vec<_> = records
        .iter()
        .map(|record| record.relay_record.body.relay_id.clone())
        .collect();

    assert!(relay_ids.contains(&identity_r2.identity_id()));
    assert!(relay_ids.contains(&identity_r3.identity_id()));

    r1_handle.abort();
}

#[tokio::test]
async fn relay_mesh_exploration_learns_relays_through_a_known_relay() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r1 = tempdir().unwrap();
    let dir_r2 = tempdir().unwrap();
    let dir_r3 = tempdir().unwrap();

    let identity_r1 = NodeIdentity::generate();
    let identity_r2 = NodeIdentity::generate();
    let identity_r3 = NodeIdentity::generate();
    let r1_id = identity_r1.identity_id();
    let r3_id = identity_r3.identity_id();

    let store_r3 = make_store(dir_r3.path());
    let mut node_r3 = NetworkNode::new_tcp(identity_r3, store_r3, relay_config()).unwrap();
    node_r3.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r3 = wait_for_listener(node_r3).await;
    let record_seen_at = unix_now();
    let r3_record = node_r3
        .local_relay_record(record_seen_at)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r2 = make_store(dir_r2.path());
    store_r2
        .record_relay_record(r3_record, record_seen_at)
        .unwrap();
    let mut node_r2 = NetworkNode::new_tcp(identity_r2, store_r2, relay_config()).unwrap();
    node_r2.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r2 = wait_for_listener(node_r2).await;
    let r2_addr = listener_with_peer(&node_r2.listeners()[0], node_r2.local_peer_id());

    let mut r1_config = relay_config();
    r1_config.effective_bootstrap_relays = vec![r2_addr.to_string()];
    let store_r1 = make_store(dir_r1.path());
    let mut node_r1 = NetworkNode::new_tcp(identity_r1, store_r1, r1_config).unwrap();
    node_r1.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r1 = wait_for_listener(node_r1).await;
    node_r1
        .connect_peer_multiaddr(&r2_addr.to_string())
        .unwrap();

    let r1_handle = tokio::spawn(async move {
        node_r1.run_event_loop().await;
    });
    let r2_handle = tokio::spawn(async move {
        node_r2.run_event_loop().await;
    });
    let r3_handle = tokio::spawn(async move {
        node_r3.run_event_loop().await;
    });

    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let now = unix_now();
            let r1_store = make_store(dir_r1.path());
            let r3_store = make_store(dir_r3.path());
            let r1_records = r1_store.load_relay_records(now).unwrap();
            let r3_records = r3_store.load_relay_records(now).unwrap();
            let r1_learned_r3 = r1_records
                .iter()
                .any(|record| record.relay_record.body.relay_id == r3_id);
            let r3_learned_r1 = r3_records
                .iter()
                .any(|record| record.relay_record.body.relay_id == r1_id);

            if r1_learned_r3 && r3_learned_r1 {
                return;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for relay mesh exploration");

    r1_handle.abort();
    r2_handle.abort();
    r3_handle.abort();
}

#[tokio::test]
async fn identity_provider_query_forwarding_finds_home_relay_provider() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r1 = tempdir().unwrap();
    let dir_r2 = tempdir().unwrap();
    let dir_tim = tempdir().unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_id = alice_identity.identity_id();
    let alice_update_log = signed_profile_log(&alice_identity, b"profile through forwarded query");

    let identity_r1 = NodeIdentity::generate();
    let identity_r2 = NodeIdentity::generate();
    let identity_tim = NodeIdentity::generate();

    let store_r1 = make_store(dir_r1.path());
    let mut node_r1 = NetworkNode::new_tcp(identity_r1, store_r1, relay_config()).unwrap();
    node_r1
        .store_verified_update_log(alice_id.clone(), alice_update_log.clone())
        .unwrap();
    node_r1.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r1 = wait_for_listener(node_r1).await;
    let r1_peer = *node_r1.local_peer_id();
    let record_seen_at = unix_now();
    let r1_record = node_r1
        .local_relay_record(record_seen_at)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r2 = make_store(dir_r2.path());
    store_r2
        .record_relay_record(r1_record, record_seen_at)
        .unwrap();
    let mut node_r2 = NetworkNode::new_tcp(identity_r2, store_r2, relay_config()).unwrap();
    node_r2.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r2 = wait_for_listener(node_r2).await;
    let r2_addr = listener_with_peer(&node_r2.listeners()[0], node_r2.local_peer_id());

    let mut tim_config = no_mdns_config();
    tim_config.effective_bootstrap_relays = vec![r2_addr.to_string()];
    let store_tim = make_store(dir_tim.path());
    let mut tim = NetworkNode::new_tcp(identity_tim, store_tim, tim_config).unwrap();
    tim.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut tim = wait_for_listener(tim).await;
    tim.connect_peer_multiaddr(&r2_addr.to_string()).unwrap();

    let r1_handle = tokio::spawn(async move {
        node_r1.run_event_loop().await;
    });
    let r2_handle = tokio::spawn(async move {
        node_r2.run_event_loop().await;
    });

    let expected_log = alice_update_log.clone();
    let provider = tokio::time::timeout(Duration::from_secs(30), async {
        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < settle_deadline {
            if let Ok(event) =
                tokio::time::timeout(Duration::from_millis(100), tim.next_event()).await
            {
                tim.handle_swarm_event(event);
            }
        }

        let mut query_interval = tokio::time::interval(Duration::from_secs(2));
        query_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = query_interval.tick() => {
                    tim.find_update_log_providers(&alice_id);
                }
                event = tim.next_event() => {
                    tim.handle_swarm_event(event);
                    if let Some(provider) = tim.take_discovered_update_log_provider(&alice_id) {
                        return provider;
                    }
                }
            }
        }
    })
    .await
    .expect("timed out waiting for forwarded identity provider query");

    assert_eq!(provider, r1_peer);

    let mut rx = tim
        .request_update_log_from(&alice_id, None, &provider)
        .expect("Tim should request Alice's update log from the forwarded provider");
    let response = loop {
        tokio::select! {
            result = &mut rx => {
                break result
                    .expect("update-log response channel should remain open")
                    .expect("update-log request should succeed");
            }
            event = tim.next_event() => {
                tim.handle_swarm_event(event);
            }
        }
    };

    assert_eq!(response.entries, expected_log);
    assert_eq!(
        tim.update_log_entries(&alice_id),
        Some(expected_log.as_slice())
    );

    r1_handle.abort();
    r2_handle.abort();
}

#[tokio::test]
async fn identity_provider_query_forwarding_crosses_multiple_relay_hops() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r1 = tempdir().unwrap();
    let dir_r2 = tempdir().unwrap();
    let dir_r3 = tempdir().unwrap();
    let dir_r4 = tempdir().unwrap();
    let dir_tim = tempdir().unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_id = alice_identity.identity_id();
    let alice_update_log = signed_profile_log(&alice_identity, b"profile through recursive query");

    let identity_r1 = NodeIdentity::generate();
    let identity_r2 = NodeIdentity::generate();
    let identity_r3 = NodeIdentity::generate();
    let identity_r4 = NodeIdentity::generate();
    let identity_tim = NodeIdentity::generate();

    let store_r4 = make_store(dir_r4.path());
    let mut node_r4 = NetworkNode::new_tcp(identity_r4, store_r4, relay_config()).unwrap();
    node_r4
        .store_verified_update_log(alice_id.clone(), alice_update_log.clone())
        .unwrap();
    node_r4.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r4 = wait_for_listener(node_r4).await;
    let r4_peer = *node_r4.local_peer_id();
    let record_seen_at = unix_now();
    let r4_record = node_r4
        .local_relay_record(record_seen_at)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r3 = make_store(dir_r3.path());
    store_r3
        .record_relay_record(r4_record, record_seen_at)
        .unwrap();
    let mut node_r3 = NetworkNode::new_tcp(identity_r3, store_r3, relay_config()).unwrap();
    node_r3.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r3 = wait_for_listener(node_r3).await;
    let r3_record = node_r3
        .local_relay_record(record_seen_at)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r2 = make_store(dir_r2.path());
    store_r2
        .record_relay_record(r3_record, record_seen_at)
        .unwrap();
    let mut node_r2 = NetworkNode::new_tcp(identity_r2, store_r2, relay_config()).unwrap();
    node_r2.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r2 = wait_for_listener(node_r2).await;
    let r2_record = node_r2
        .local_relay_record(record_seen_at)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r1 = make_store(dir_r1.path());
    store_r1
        .record_relay_record(r2_record, record_seen_at)
        .unwrap();
    let mut node_r1 = NetworkNode::new_tcp(identity_r1, store_r1, relay_config()).unwrap();
    node_r1.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r1 = wait_for_listener(node_r1).await;
    let r1_addr = listener_with_peer(&node_r1.listeners()[0], node_r1.local_peer_id());

    let mut tim_config = no_mdns_config();
    tim_config.effective_bootstrap_relays = vec![r1_addr.to_string()];
    let store_tim = make_store(dir_tim.path());
    let mut tim = NetworkNode::new_tcp(identity_tim, store_tim, tim_config).unwrap();
    tim.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut tim = wait_for_listener(tim).await;
    tim.connect_peer_multiaddr(&r1_addr.to_string()).unwrap();

    let r1_handle = tokio::spawn(async move {
        node_r1.run_event_loop().await;
    });
    let r2_handle = tokio::spawn(async move {
        node_r2.run_event_loop().await;
    });
    let r3_handle = tokio::spawn(async move {
        node_r3.run_event_loop().await;
    });
    let r4_handle = tokio::spawn(async move {
        node_r4.run_event_loop().await;
    });

    let expected_log = alice_update_log.clone();
    let provider = tokio::time::timeout(Duration::from_secs(45), async {
        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < settle_deadline {
            if let Ok(event) =
                tokio::time::timeout(Duration::from_millis(100), tim.next_event()).await
            {
                tim.handle_swarm_event(event);
            }
        }

        let mut query_interval = tokio::time::interval(Duration::from_secs(2));
        query_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = query_interval.tick() => {
                    tim.find_update_log_providers(&alice_id);
                }
                event = tim.next_event() => {
                    tim.handle_swarm_event(event);
                    if let Some(provider) = tim.take_discovered_update_log_provider(&alice_id) {
                        return provider;
                    }
                }
            }
        }
    })
    .await
    .expect("timed out waiting for recursive identity provider query");

    assert_eq!(provider, r4_peer);

    let mut rx = tim
        .request_update_log_from(&alice_id, None, &provider)
        .expect("Tim should request Alice's update log from the forwarded provider");
    let response = loop {
        tokio::select! {
            result = &mut rx => {
                break result
                    .expect("update-log response channel should remain open")
                    .expect("update-log request should succeed");
            }
            event = tim.next_event() => {
                tim.handle_swarm_event(event);
            }
        }
    };

    assert_eq!(response.entries, expected_log);
    assert_eq!(
        tim.update_log_entries(&alice_id),
        Some(expected_log.as_slice())
    );

    r1_handle.abort();
    r2_handle.abort();
    r3_handle.abort();
    r4_handle.abort();
}

#[tokio::test]
async fn identity_head_gossip_resolves_common_lookup_without_forwarding() {
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r1 = tempdir().unwrap();
    let dir_r2 = tempdir().unwrap();
    let dir_tim = tempdir().unwrap();

    let alice_identity = NodeIdentity::generate();
    let alice_id = alice_identity.identity_id();
    let alice_update_log = signed_profile_log(&alice_identity, b"profile through gossiped hint");

    let identity_r1 = NodeIdentity::generate();
    let identity_r2 = NodeIdentity::generate();
    let identity_tim = NodeIdentity::generate();

    let store_r1 = make_store(dir_r1.path());
    let mut node_r1 = NetworkNode::new_tcp(identity_r1, store_r1, relay_config()).unwrap();
    node_r1
        .store_verified_update_log(alice_id.clone(), alice_update_log.clone())
        .unwrap();
    node_r1.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r1 = wait_for_listener(node_r1).await;
    let r1_peer = *node_r1.local_peer_id();
    let r1_addr = listener_with_peer(&node_r1.listeners()[0], &r1_peer);

    let now = unix_now();
    let hint = signed_identity_head_hint(
        &alice_identity,
        &r1_peer,
        vec![r1_addr.to_string()],
        &alice_update_log,
        now,
    );
    node_r1.record_identity_head_hint(hint).unwrap();

    let r1_record = node_r1
        .local_relay_record(now)
        .unwrap()
        .expect("relay-mode node should expose a local relay record");

    let store_r2 = make_store(dir_r2.path());
    store_r2.record_relay_record(r1_record, now).unwrap();
    let mut node_r2 = NetworkNode::new_tcp(identity_r2, store_r2, relay_config()).unwrap();
    node_r2.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut node_r2 = wait_for_listener(node_r2).await;
    let r2_addr = listener_with_peer(&node_r2.listeners()[0], node_r2.local_peer_id());

    let mut tim_config = no_mdns_config();
    tim_config.effective_bootstrap_relays = vec![r2_addr.to_string()];
    let store_tim = make_store(dir_tim.path());
    let mut tim = NetworkNode::new_tcp(identity_tim, store_tim, tim_config).unwrap();
    tim.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let mut tim = wait_for_listener(tim).await;
    tim.connect_peer_multiaddr(&r2_addr.to_string()).unwrap();

    let r1_handle = tokio::spawn(async move {
        node_r1.run_event_loop().await;
    });
    let r2_handle = tokio::spawn(async move {
        node_r2.run_event_loop().await;
    });

    let provider = tokio::time::timeout(Duration::from_secs(30), async {
        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        while tokio::time::Instant::now() < settle_deadline {
            if let Ok(event) =
                tokio::time::timeout(Duration::from_millis(100), tim.next_event()).await
            {
                tim.handle_swarm_event(event);
            }
        }

        let mut query_interval = tokio::time::interval(Duration::from_secs(2));
        query_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = query_interval.tick() => {
                    tim.find_update_log_providers(&alice_id);
                }
                event = tim.next_event() => {
                    tim.handle_swarm_event(event);
                    if let Some(provider) = tim.take_discovered_update_log_provider(&alice_id) {
                        return provider;
                    }
                }
            }
        }
    })
    .await
    .expect("timed out waiting for gossiped identity-head hint");

    assert_eq!(provider, r1_peer);

    let mut rx = tim
        .request_update_log_from(&alice_id, None, &provider)
        .expect("Tim should request Alice's update log from the hinted provider");
    let response = loop {
        tokio::select! {
            result = &mut rx => {
                break result
                    .expect("update-log response channel should remain open")
                    .expect("update-log request should succeed");
            }
            event = tim.next_event() => {
                tim.handle_swarm_event(event);
            }
        }
    };

    assert_eq!(response.entries, alice_update_log);
    assert_eq!(
        tim.update_log_entries(&alice_id),
        Some(alice_update_log.as_slice())
    );

    r1_handle.abort();
    r2_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_nodes_deliver_ingress_over_p2p() {
    // Regression test for #195: an ingress envelope addressed to a remote
    // identity must queue on the RECIPIENT's daemon with the recipient's
    // identity stamped on the record. The old HTTP delivery path posted to
    // the loopback URL in the recipient's reachability record, which fed the
    // envelope back to the sender's own daemon and stamped the sender's
    // identity as recipient.
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_r = tempdir().unwrap();
    let dir_s = tempdir().unwrap();

    let recipient = NodeIdentity::generate();
    let recipient_identity = recipient.identity_id();
    let sender = NodeIdentity::generate();

    // The envelope names the recipient identity; the wrapped key does not
    // need to be decryptable for the queueing path under test.
    let (recipient_key, _recipient_private) = jolt_core::generate_identity_encryption_keypair(
        recipient_identity.clone(),
        "key-1".to_string(),
        unix_now(),
    );
    let envelope = jolt_core::EncryptedObjectEnvelope::encrypt(
        sender.public_key_bytes(),
        sender.identity_id(),
        br#"{"schema":"spoke.follow_request.v1","id":"follow_req_test"}"#,
        "application/json".to_string(),
        Some("spoke.follow_request.v1".to_string()),
        vec![jolt_core::EncryptedObjectRecipient {
            identity: recipient_identity.clone(),
            key: recipient_key,
        }],
        unix_now(),
        |bytes| sender.sign(bytes),
    )
    .unwrap()
    .to_bytes()
    .unwrap();

    let store_r = make_store(dir_r.path());
    let mut node_r = NetworkNode::new_tcp(recipient, store_r, no_mdns_config()).unwrap();
    node_r.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
    let node_r = wait_for_listener(node_r).await;
    let recipient_peer = *node_r.local_peer_id();
    let addr_r = node_r.listeners()[0].clone();

    let store_s = make_store(dir_s.path());
    let mut node_s = NetworkNode::new_tcp(sender, store_s, no_mdns_config()).unwrap();
    node_s.dial(addr_r).unwrap();

    let (tx_result, rx_result) = tokio::sync::oneshot::channel();

    let mut node_r = node_r;
    let node_r_handle = tokio::spawn(async move {
        node_r.run_event_loop().await;
    });

    let node_s_handle = tokio::spawn(async move {
        let peer = loop {
            let event = tokio::time::timeout(Duration::from_secs(1), node_s.next_event()).await;
            if let Ok(ev) = event {
                node_s.handle_swarm_event(ev);
            }
            if let Some(peer) = node_s.connected_peers().first().cloned() {
                break peer;
            }
        };
        assert_eq!(peer, recipient_peer, "sender connected to the recipient");

        let (tx, mut rx) = tokio::sync::oneshot::channel();
        node_s.send_ingress_to_peer(&peer, "p2p-live".to_string(), envelope, None, tx);

        loop {
            tokio::select! {
                result = &mut rx => {
                    let _ = tx_result.send(result);
                    return;
                }
                event = node_s.next_event() => {
                    node_s.handle_swarm_event(event);
                }
            }
        }
    });

    let record = tokio::time::timeout(Duration::from_secs(15), rx_result)
        .await
        .expect("timed out waiting for ingress delivery")
        .expect("sender task dropped result channel")
        .expect("oneshot closed")
        .expect("ingress delivery failed");

    assert_eq!(
        record.recipient_identity,
        recipient_identity.to_string(),
        "record must be stamped with the addressed recipient, not the sender"
    );
    assert_eq!(record.receiver_id, "p2p-live");
    assert!(record.ingress_id.starts_with("ing_"));

    node_r_handle.abort();
    node_s_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ingress_delivery_to_unreachable_peer_retries_then_fails() {
    // Transient transport failures are retried inside the daemon (idle-timeout
    // closes and NAT path changes kill connections between sends). When the
    // peer stays unreachable the retries must exhaust and surface an error
    // instead of hanging or succeeding silently.
    let _ = tracing_subscriber::fmt().with_env_filter("info").try_init();
    let _guard = integration_test_lock().lock().await;

    let dir_s = tempdir().unwrap();
    let sender = NodeIdentity::generate();
    let unreachable_peer = NodeIdentity::generate().peer_id();

    let store_s = make_store(dir_s.path());
    let mut node_s = NetworkNode::new_tcp(sender, store_s, no_mdns_config()).unwrap();

    let (tx, mut rx) = tokio::sync::oneshot::channel();
    node_s.send_ingress_to_peer(
        &unreachable_peer,
        "p2p-live".to_string(),
        vec![1, 2, 3],
        None,
        tx,
    );

    let result = tokio::time::timeout(Duration::from_secs(20), async move {
        loop {
            tokio::select! {
                result = &mut rx => return result,
                event = node_s.next_event() => node_s.handle_swarm_event(event),
            }
        }
    })
    .await
    .expect("delivery to an unreachable peer must fail within the timeout")
    .expect("oneshot closed");

    let err = result.expect_err("delivery to an unreachable peer cannot succeed");
    assert!(
        err.to_string().contains("could not deliver ingress envelope"),
        "unexpected error: {err}"
    );
}
