use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tracing::info;

use dweb_core::ContentId;
use dweb_identity::verify_signature;
use dweb_identity::NodeIdentity;
use dweb_network::{ContentResponse, NetworkConfig, NetworkNode};
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run(
    content_id_str: &str,
    output: Option<PathBuf>,
    dial: Option<String>,
    bootstrap: Vec<String>,
) -> Result<()> {
    let content_id: ContentId = content_id_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid ContentId: {e}"))?;

    let mut node = setup_node(&bootstrap).await?;

    info!("Fetching: {content_id}");

    connect_to_peers(&mut node, dial.as_deref(), &bootstrap).await?;

    // Try connected peers first
    if let Some(resp) = try_fetch_from_peers(&mut node, &content_id).await {
        return save_and_verify(resp, &content_id, output, content_id_str);
    }

    // Fall back to DHT provider discovery
    if let Some(resp) = fetch_via_dht(&mut node, &content_id).await {
        return save_and_verify(resp, &content_id, output, content_id_str);
    }

    anyhow::bail!("Content not found on any peer or via DHT")
}

async fn setup_node(bootstrap: &[String]) -> Result<NetworkNode> {
    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    let identity = NodeIdentity::generate();
    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;

    let mut net_config = NetworkConfig::default();
    if !bootstrap.is_empty() {
        net_config.bootstrap_peers = bootstrap
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();
    }

    let mut node = NetworkNode::new(identity, store, net_config).await?;
    node.listen_on("/ip4/0.0.0.0/tcp/0")?;
    node.listen_on("/ip4/0.0.0.0/udp/0/quic-v1")?;

    Ok(node)
}

async fn connect_to_peers(
    node: &mut NetworkNode,
    dial: Option<&str>,
    bootstrap: &[String],
) -> Result<()> {
    if let Some(addr) = dial {
        info!("Dialing peer at {addr}");
        let multiaddr: dweb_network::Multiaddr = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid multiaddr: {e}"))?;
        node.dial(multiaddr)?;
    } else if !bootstrap.is_empty() {
        info!("Bootstrapping into DHT...");
        let addrs: Vec<dweb_network::Multiaddr> =
            bootstrap.iter().filter_map(|s| s.parse().ok()).collect();
        node.bootstrap_dht(&addrs)?;
    } else {
        info!("Discovering peers via mDNS...");
    }

    // Wait for at least one peer connection
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        pump_events(node, Duration::from_millis(500)).await;
        if !node.connected_peers().is_empty() {
            info!("Found {} peer(s)", node.connected_peers().len());
            return Ok(());
        }
    }

    anyhow::bail!("Timed out waiting for peers. Try --dial or --bootstrap.")
}

async fn try_fetch_from_peers(
    node: &mut NetworkNode,
    content_id: &ContentId,
) -> Option<ContentResponse> {
    let rx = node.request_content(content_id).ok()?;
    let mut rx = rx;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        if tokio::time::Instant::now() > deadline {
            return None;
        }
        tokio::select! {
            result = &mut rx => {
                return match result {
                    Ok(Ok(resp)) if !resp.data.is_empty() => Some(resp),
                    _ => None,
                };
            }
            event = node.next_event() => {
                node.handle_swarm_event(event);
            }
        }
    }
}

async fn fetch_via_dht(
    node: &mut NetworkNode,
    content_id: &ContentId,
) -> Option<ContentResponse> {
    info!("Content not on connected peers, querying DHT for providers...");
    let _query_id = node.find_providers(content_id);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut provider_peer: Option<dweb_network::PeerId> = None;

    while tokio::time::Instant::now() < deadline {
        pump_events(node, Duration::from_millis(200)).await;

        // Wait for DHT to discover a provider
        if provider_peer.is_none() {
            if let Some(peer) = node.take_discovered_provider(content_id) {
                info!("DHT discovered provider: {peer}");
                provider_peer = Some(peer);
            }
        }

        // Once provider is connected AND relay circuit is ready, send request
        if let Some(ref provider) = provider_peer {
            let connected = node.connected_peers().contains(provider);
            let relay_ready = node.has_ready_relay();

            if connected && relay_ready {
                info!("Requesting content from provider {provider}...");
                if let Some(resp) = try_fetch_from_specific_peer(node, content_id, provider).await
                {
                    return Some(resp);
                }
                // If this provider failed, try next one
                provider_peer = node.take_discovered_provider(content_id);
            }
        }
    }

    None
}

async fn try_fetch_from_specific_peer(
    node: &mut NetworkNode,
    content_id: &ContentId,
    peer: &dweb_network::PeerId,
) -> Option<ContentResponse> {
    let rx = node.request_content_from(content_id, peer).ok()?;
    let mut rx = rx;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        if tokio::time::Instant::now() > deadline {
            return None;
        }
        tokio::select! {
            result = &mut rx => {
                return match result {
                    Ok(Ok(resp)) if !resp.data.is_empty() => Some(resp),
                    _ => None,
                };
            }
            event = node.next_event() => {
                node.handle_swarm_event(event);
            }
        }
    }
}

fn save_and_verify(
    response: ContentResponse,
    content_id: &ContentId,
    output: Option<PathBuf>,
    content_id_str: &str,
) -> Result<()> {
    if !content_id.verify(&response.data) {
        anyhow::bail!("Content verification failed: hash mismatch");
    }
    info!("Hash verified: content matches ContentId");

    if let Ok(valid) =
        verify_signature(&response.publisher_key, &response.data, &response.signature)
    {
        if valid {
            info!("Signature verified");
        } else {
            anyhow::bail!("Signature verification failed");
        }
    }

    info!("Content auto-cached for re-sharing");

    let output_path = output.unwrap_or_else(|| PathBuf::from(content_id_str));
    std::fs::write(&output_path, &response.data)?;
    info!("Saved to: {}", output_path.display());
    println!(
        "Saved {} bytes to {}",
        response.data.len(),
        output_path.display()
    );

    Ok(())
}

async fn pump_events(node: &mut NetworkNode, timeout: Duration) {
    if let Ok(ev) = tokio::time::timeout(timeout, node.next_event()).await {
        node.handle_swarm_event(ev);
    }
}
