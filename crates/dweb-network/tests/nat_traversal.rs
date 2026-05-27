//! NAT traversal tests using patchbay network namespace simulation.
//!
//! These tests create realistic network topologies (Home NAT, CGNAT, etc.)
//! and verify connectivity across them.
//!
//! Requires Linux with unprivileged user namespaces enabled.
//! Run with: cargo test -p dweb-network --test nat_traversal

#![cfg(target_os = "linux")]

use std::net::{IpAddr, SocketAddr};
use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result};
use patchbay::{Lab, Nat};
use tokio::sync::mpsc;

use dweb_identity::NodeIdentity;
use dweb_network::{DaemonHandle, NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};

fn init_patchbay() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        patchbay::init_userns().expect("user namespace bootstrap");
    });
}

/// Test A: Two devices on the same public network (no NAT).
/// Verify direct TCP connectivity.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_lan_direct_transfer() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    let dc = lab.add_router("dc").build().await?;

    let dev1 = lab
        .add_device("node1")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let dev2 = lab
        .add_device("node2")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let dev1_ip = dev1.ip().context("no node1 ip")?;
    let dev2_ip = dev2.ip().context("no node2 ip")?;

    // Both should be on the same subnet
    assert_ne!(dev1_ip, dev2_ip, "devices should have different IPs");

    // Verify direct TCP works between LAN peers
    let listen_addr = SocketAddr::new(IpAddr::V4(dev1_ip), 9000);

    dev1.spawn(move |_| async move {
        let listener = tokio::net::TcpListener::bind(listen_addr).await?;
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n]).await?;
        anyhow::Ok(())
    })?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let echoed = dev2
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(listen_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"lan direct").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(echoed, b"lan direct", "LAN TCP echo should work");

    Ok(())
}

/// Test B: Two devices behind separate Home NATs.
/// Outbound TCP from NATed device to public server should work.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_home_nat_connectivity() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    let home1 = lab.add_router("home1").nat(Nat::Home).build().await?;
    let home2 = lab.add_router("home2").nat(Nat::Home).build().await?;

    let dev1 = lab
        .add_device("node1")
        .iface("eth0", home1.id(), None)
        .build()
        .await?;

    let dev2 = lab
        .add_device("node2")
        .iface("eth0", home2.id(), None)
        .build()
        .await?;

    // Verify both devices got IPs behind NAT
    assert!(dev1.ip().is_some(), "node1 should have an IP");
    assert!(dev2.ip().is_some(), "node2 should have an IP");

    // Verify the IPs are different (different NATs)
    assert_ne!(
        dev1.ip(),
        dev2.ip(),
        "nodes should be on different networks"
    );

    // Both can reach a server on the public backbone
    let dc = lab.add_router("dc").build().await?;
    let server = lab
        .add_device("server")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let server_ip = server.ip().context("no server ip")?;
    let server_addr = SocketAddr::new(IpAddr::V4(server_ip), 9000);

    server.spawn(move |_| async move {
        let listener = tokio::net::TcpListener::bind(server_addr).await?;
        // Accept two connections (one from each NATed device)
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n]).await?;
        }
        anyhow::Ok(())
    })?;

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Device 1 connects through home1 NAT
    let echoed1 = dev1
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(server_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"home1").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(echoed1, b"home1", "TCP through Home NAT 1 should work");

    // Device 2 connects through home2 NAT
    let echoed2 = dev2
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(server_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"home2").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(echoed2, b"home2", "TCP through Home NAT 2 should work");

    Ok(())
}

/// Test C: Device behind CGNAT (carrier-grade NAT).
/// Outbound TCP to public server should still work.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_cgnat_topology() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    let isp = lab.add_router("isp").nat(Nat::Cgnat).build().await?;
    let dc = lab.add_router("dc").build().await?;

    let client = lab
        .add_device("client")
        .iface("eth0", isp.id(), None)
        .build()
        .await?;

    let server = lab
        .add_device("server")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let _client_ip = client.ip().context("no client ip")?;
    let server_ip = server.ip().context("no server ip")?;

    let server_addr = SocketAddr::new(IpAddr::V4(server_ip), 9000);

    server.spawn(move |_| async move {
        let listener = tokio::net::TcpListener::bind(server_addr).await?;
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n]).await?;
        anyhow::Ok(())
    })?;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let echoed = client
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(server_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"cgnat test").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(echoed, b"cgnat test", "TCP should work through CGNAT");

    Ok(())
}

/// Test D: Corporate symmetric NAT (hardest for hole punching).
/// TCP outbound should still work.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_corporate_symmetric_nat() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    let corp = lab.add_router("corp").nat(Nat::Corporate).build().await?;

    let dc = lab.add_router("dc").build().await?;

    let employee = lab
        .add_device("employee")
        .iface("eth0", corp.id(), None)
        .build()
        .await?;

    let server = lab
        .add_device("server")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let server_addr = SocketAddr::new(IpAddr::V4(server.ip().unwrap()), 80);

    server.spawn(move |_| async move {
        let listener = tokio::net::TcpListener::bind(server_addr).await?;
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n]).await?;
        anyhow::Ok(())
    })?;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let echoed = employee
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(server_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"corp nat").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(
        echoed, b"corp nat",
        "TCP on port 80 should work through corporate NAT"
    );

    Ok(())
}

/// Test E: Double NAT -- Home behind CGNAT (common mobile carrier scenario).
/// Client must traverse two layers of NAT to reach server.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_double_nat_cgnat_plus_home() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    // ISP-level CGNAT
    let isp = lab.add_router("isp").nat(Nat::Cgnat).build().await?;
    // Home router behind ISP
    let home = lab
        .add_router("home")
        .nat(Nat::Home)
        .upstream(isp.id())
        .build()
        .await?;
    // Public datacenter
    let dc = lab.add_router("dc").build().await?;

    let client = lab
        .add_device("client")
        .iface("eth0", home.id(), None)
        .build()
        .await?;

    let server = lab
        .add_device("server")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let server_ip = server.ip().context("no server ip")?;
    let server_addr = SocketAddr::new(IpAddr::V4(server_ip), 9000);

    server.spawn(move |_| async move {
        let listener = tokio::net::TcpListener::bind(server_addr).await?;
        let (mut stream, _) = listener.accept().await?;
        let mut buf = vec![0u8; 64];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
        tokio::io::AsyncWriteExt::write_all(&mut stream, &buf[..n]).await?;
        anyhow::Ok(())
    })?;

    tokio::time::sleep(Duration::from_millis(200)).await;

    let echoed = client
        .spawn(move |_| async move {
            let mut stream = tokio::net::TcpStream::connect(server_addr).await?;
            tokio::io::AsyncWriteExt::write_all(&mut stream, b"double nat").await?;
            let mut buf = vec![0u8; 64];
            let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await?;
            anyhow::Ok(buf[..n].to_vec())
        })?
        .await??;

    assert_eq!(echoed, b"double nat", "TCP should work through double NAT");

    Ok(())
}

/// Test F: Two dweb nodes on the same LAN, content transfer via direct dial.
/// Publisher publishes a file, fetcher connects directly and fetches it.
/// Uses TCP transport (not iroh) since patchbay namespaces have no internet.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_lan_dweb_content_transfer() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;

    let dc = lab.add_router("dc").build().await?;

    let publisher_dev = lab
        .add_device("publisher")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let fetcher_dev = lab
        .add_device("fetcher")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let publisher_ip = publisher_dev.ip().context("no publisher ip")?;
    let publisher_port: u16 = 5001;

    // Coordination: publisher sends (content_id, peer_id) to fetcher
    let (info_tx, mut info_rx) = mpsc::channel::<(String, String)>(1);

    // Publisher: create node with TCP on fixed port, publish content, run daemon loop
    let _publisher_handle = publisher_dev.spawn(move |_| async move {
        let dir = tempfile::tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

        node.listen_on(&format!("/ip4/{}/tcp/{}", publisher_ip, publisher_port))
            .unwrap();

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"patchbay LAN transfer test").unwrap();
        let cid = node.publish_file(&test_file).unwrap();

        let peer_id = node.local_peer_id().to_string();
        info_tx.send((cid.to_string(), peer_id)).await.unwrap();

        let (_cmd_tx, cmd_rx) = mpsc::channel(16);
        node.run_daemon_loop(cmd_rx).await;
    })?;

    let (content_id_str, publisher_peer_id) =
        info_rx.recv().await.context("publisher didn't send info")?;

    let publisher_multiaddr = format!(
        "/ip4/{}/tcp/{}/p2p/{}",
        publisher_ip, publisher_port, publisher_peer_id
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Fetcher: create node, dial publisher directly, fetch content
    let fetched_data = fetcher_dev
        .spawn(move |_| async move {
            let dir = tempfile::tempdir().unwrap();
            let identity = NodeIdentity::generate();
            let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            let mut node =
                NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

            node.listen_on("/ip4/0.0.0.0/tcp/0").unwrap();
            node.dial(publisher_multiaddr.parse::<libp2p::Multiaddr>().unwrap())
                .unwrap();

            let (cmd_tx, cmd_rx) = mpsc::channel(16);
            let daemon = DaemonHandle::new(cmd_tx);

            tokio::spawn(async move {
                node.run_daemon_loop(cmd_rx).await;
            });

            // Give time for connection to establish
            tokio::time::sleep(Duration::from_secs(2)).await;

            let result =
                tokio::time::timeout(Duration::from_secs(15), daemon.fetch(content_id_str)).await;

            match result {
                Ok(Ok(fetch_result)) => Some(fetch_result.data),
                Ok(Err(e)) => {
                    eprintln!("fetch error: {e}");
                    None
                }
                Err(_) => {
                    eprintln!("fetch timed out");
                    None
                }
            }
        })?
        .await?;

    assert_eq!(
        fetched_data.as_deref(),
        Some(b"patchbay LAN transfer test".as_slice()),
        "fetched content should match published content"
    );

    Ok(())
}

/// Test G: Three dweb nodes on the same public network. Publisher publishes content,
/// announces to DHT via bootstrap. Fetcher discovers content via DHT through bootstrap
/// and fetches it from the publisher. Tests the full publish-discover-fetch DHT cycle.
#[tokio::test(flavor = "current_thread")]
#[ignore = "manual patchbay test: requires Linux network namespace support"]
async fn test_three_node_dht_discovery() -> Result<()> {
    init_patchbay();

    let lab = Lab::new().await?;
    let dc = lab.add_router("dc").build().await?;

    let bootstrap_dev = lab
        .add_device("bootstrap")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;
    let publisher_dev = lab
        .add_device("publisher")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;
    let fetcher_dev = lab
        .add_device("fetcher")
        .iface("eth0", dc.id(), None)
        .build()
        .await?;

    let bootstrap_ip = bootstrap_dev.ip().context("no bootstrap ip")?;
    let bootstrap_port: u16 = 4001;

    // Coordination channels
    let (bootstrap_peer_tx, mut bootstrap_peer_rx) = mpsc::channel::<String>(1);
    let (content_id_tx, mut content_id_rx) = mpsc::channel::<String>(1);

    // 1. Bootstrap node: listens on fixed port
    let _bootstrap_handle = bootstrap_dev.spawn(move |_| async move {
        let dir = tempfile::tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

        node.listen_on(&format!("/ip4/{}/tcp/{}", bootstrap_ip, bootstrap_port))
            .unwrap();

        bootstrap_peer_tx
            .send(node.local_peer_id().to_string())
            .await
            .unwrap();

        let (_cmd_tx, cmd_rx) = mpsc::channel(16);
        node.run_daemon_loop(cmd_rx).await;
    })?;

    let bootstrap_peer_id = bootstrap_peer_rx
        .recv()
        .await
        .context("bootstrap didn't report peer ID")?;
    let bootstrap_multiaddr = format!(
        "/ip4/{}/tcp/{}/p2p/{}",
        bootstrap_ip, bootstrap_port, bootstrap_peer_id
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    // 2. Publisher: dials bootstrap, publishes content (DHT announces)
    let baddr = bootstrap_multiaddr.clone();
    let _publisher_handle = publisher_dev.spawn(move |_| async move {
        let dir = tempfile::tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let mut node = NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

        node.listen_on("/ip4/0.0.0.0/tcp/0").unwrap();
        node.dial(baddr.parse::<libp2p::Multiaddr>().unwrap())
            .unwrap();

        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, b"DHT discovery test content").unwrap();
        let cid = node.publish_file(&test_file).unwrap();
        content_id_tx.send(cid.to_string()).await.unwrap();

        let (_cmd_tx, cmd_rx) = mpsc::channel(16);
        node.run_daemon_loop(cmd_rx).await;
    })?;

    let content_id = content_id_rx
        .recv()
        .await
        .context("publisher didn't report content ID")?;

    // Give publisher time to connect to bootstrap and announce to DHT
    tokio::time::sleep(Duration::from_secs(3)).await;

    // 3. Fetcher: dials bootstrap, discovers content via DHT, fetches from publisher
    let baddr = bootstrap_multiaddr.clone();
    let fetched_data = fetcher_dev
        .spawn(move |_| async move {
            let dir = tempfile::tempdir().unwrap();
            let identity = NodeIdentity::generate();
            let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
            let mut node =
                NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap();

            node.listen_on("/ip4/0.0.0.0/tcp/0").unwrap();
            node.dial(baddr.parse::<libp2p::Multiaddr>().unwrap())
                .unwrap();

            let (cmd_tx, cmd_rx) = mpsc::channel(16);
            let daemon = DaemonHandle::new(cmd_tx);

            tokio::spawn(async move {
                node.run_daemon_loop(cmd_rx).await;
            });

            // Wait for DHT routing to propagate
            tokio::time::sleep(Duration::from_secs(3)).await;

            let result =
                tokio::time::timeout(Duration::from_secs(20), daemon.fetch(content_id)).await;

            match result {
                Ok(Ok(fetch_result)) => Some(fetch_result.data),
                Ok(Err(e)) => {
                    eprintln!("DHT fetch error: {e}");
                    None
                }
                Err(_) => {
                    eprintln!("DHT fetch timed out");
                    None
                }
            }
        })?
        .await?;

    assert_eq!(
        fetched_data.as_deref(),
        Some(b"DHT discovery test content".as_slice()),
        "content should transfer via DHT discovery through bootstrap"
    );

    Ok(())
}
