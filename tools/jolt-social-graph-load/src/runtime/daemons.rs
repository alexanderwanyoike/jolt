use super::*;

pub(super) struct RunningDaemon {
    pub(super) handle: DaemonHandle,
    pub(super) peer_id: String,
    pub(super) listen_addr: String,
    task: JoinHandle<()>,
}

impl RunningDaemon {
    pub(super) async fn stop(&mut self) {
        let _ = self.handle.shutdown().await;
        let _ = (&mut self.task).await;
    }

    pub(super) async fn shutdown(mut self) {
        self.stop().await;
    }
}

pub(super) struct DaemonSpec<'a> {
    pub(super) domain: &'a str,
    pub(super) index: usize,
    pub(super) provider_record_capacity: usize,
    pub(super) cache_max_bytes: u64,
    pub(super) listen_port: u16,
}

pub(super) async fn start_daemon(
    workdir: &Path,
    seed: u64,
    spec: DaemonSpec<'_>,
) -> anyhow::Result<RunningDaemon> {
    let root = workdir.join(format!("{}-{}", spec.domain, spec.index));
    std::fs::create_dir_all(&root)?;
    let identity =
        NodeIdentity::from_signing_key_bytes(&deterministic_bytes(seed, spec.domain, spec.index))
            .map_err(|error| {
            anyhow::anyhow!("create {} identity {}: {error}", spec.domain, spec.index)
        })?;
    let identity_id = identity.identity_id().to_string();
    let store = ContentStore::open(
        &root,
        CacheConfig {
            max_size_bytes: spec.cache_max_bytes,
        },
    )?;
    let mut node = NetworkNode::new_tcp(
        identity,
        store,
        NetworkConfig {
            enable_mdns: false,
            provider_record_capacity: (spec.provider_record_capacity > 0)
                .then_some(spec.provider_record_capacity),
            ..NetworkConfig::test_config()
        },
    )?;
    let listen_address = format!("/ip4/127.0.0.1/tcp/{}", spec.listen_port);
    node.listen_on(&listen_address)?;
    node = wait_for_listener(node).await?;
    let peer_id = node.local_peer_id().to_string();
    let listen_addr = node
        .listeners()
        .first()
        .context("daemon has no listener")?
        .to_string();
    let (tx, rx) = mpsc::channel::<DaemonCommand>(4_096);
    let handle = DaemonHandle::new_with_local_identity(tx, identity_id);
    let task = tokio::spawn(async move { node.run_daemon_loop(rx).await });
    Ok(RunningDaemon {
        handle,
        peer_id,
        listen_addr,
        task,
    })
}

async fn wait_for_listener(mut node: NetworkNode) -> anyhow::Result<NetworkNode> {
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
    .context("timed out waiting for daemon listener")
}

pub(super) fn listener_socket(address: &str) -> anyhow::Result<SocketAddr> {
    let multiaddr: libp2p::Multiaddr = address.parse()?;
    let mut ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let mut port = None;
    for protocol in multiaddr.iter() {
        match protocol {
            Protocol::Ip4(value) => ip = IpAddr::V4(value),
            Protocol::Ip6(value) => ip = IpAddr::V6(value),
            Protocol::Tcp(value) => port = Some(value),
            _ => {}
        }
    }
    Ok(SocketAddr::new(
        ip,
        port.context("listener has no TCP port")?,
    ))
}

pub(super) async fn connect_reader_to_providers(
    reader: &DaemonHandle,
    providers: &[RunningDaemon],
    links: &[ShapedLink],
) -> anyhow::Result<()> {
    for (provider, link) in providers.iter().zip(links) {
        reader
            .connect_peer(format!(
                "/ip4/127.0.0.1/tcp/{}/p2p/{}",
                link.port, provider.peer_id
            ))
            .await
            .with_context(|| format!("connect reader to provider {}", provider.peer_id))?;
    }
    wait_for_peer_count(reader, providers.len()).await
}

async fn wait_for_peer_count(handle: &DaemonHandle, expected: usize) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if handle
                .status()
                .await
                .is_ok_and(|status| status.connected_peers >= expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("timed out connecting benchmark daemons")
}

pub(super) async fn wait_for_peer_absence(
    handle: &DaemonHandle,
    absent_peer_ids: &[String],
) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if handle.peers().await.is_ok_and(|peers| {
                peers
                    .iter()
                    .all(|peer| !absent_peer_ids.contains(&peer.peer_id))
            }) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("timed out disconnecting benchmark providers")
}
