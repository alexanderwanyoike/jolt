use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tracing::info;

use dweb_core::ContentId;
use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::{NetworkConfig, NetworkNode};
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

    info!("Fetching: {content_id}");

    if let Some(ref addr) = dial {
        // Direct dial mode (manual address)
        info!("Dialing peer at {addr}");
        let multiaddr: dweb_network::Multiaddr = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid multiaddr: {e}"))?;
        node.dial(multiaddr)?;
    } else if !bootstrap.is_empty() {
        // DHT mode: bootstrap, then find providers
        info!("Bootstrapping into DHT...");
        let addrs: Vec<dweb_network::Multiaddr> =
            bootstrap.iter().filter_map(|s| s.parse().ok()).collect();
        node.bootstrap_dht(&addrs)?;
    } else {
        info!("Discovering peers via mDNS...");
    }

    // Wait for peer connection
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!(
                "Timed out waiting for peers. Try --dial or --bootstrap."
            );
        }

        let event = tokio::time::timeout(Duration::from_millis(500), node.next_event()).await;
        if let Ok(ev) = event {
            node.handle_swarm_event(ev);
        }

        if !node.connected_peers().is_empty() {
            break;
        }
    }

    info!("Found {} peer(s)", node.connected_peers().len());

    // Try fetching from connected peers first
    let rx = node.request_content(&content_id)?;

    let mut response = {
        let mut rx = rx;
        let fetch_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if tokio::time::Instant::now() > fetch_deadline {
                break dweb_network::ContentResponse {
                    data: vec![],
                    signature: vec![],
                    publisher_key: vec![],
                };
            }

            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(resp)) => break resp,
                        Ok(Err(_)) => break dweb_network::ContentResponse {
                            data: vec![], signature: vec![], publisher_key: vec![],
                        },
                        Err(_) => break dweb_network::ContentResponse {
                            data: vec![], signature: vec![], publisher_key: vec![],
                        },
                    }
                }
                event = node.next_event() => {
                    node.handle_swarm_event(event);
                }
            }
        }
    };

    // If not found on directly connected peers, try DHT provider discovery
    if response.data.is_empty() {
        info!("Content not on connected peers, querying DHT for providers...");
        let _query_id = node.find_providers(&content_id);

        let dht_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut last_peer_count = node.connected_peers().len();
        let mut retry_at = tokio::time::Instant::now() + Duration::from_secs(30); // no retry initially

        while tokio::time::Instant::now() < dht_deadline {
            let event = tokio::time::timeout(Duration::from_millis(200), node.next_event()).await;
            if let Ok(ev) = event {
                node.handle_swarm_event(ev);
            }

            // When a new peer connects, schedule a retry after a short delay
            // to let the connection fully establish
            let current_peers = node.connected_peers().len();
            if current_peers > last_peer_count {
                info!("New peer connected via DHT, will request content shortly...");
                last_peer_count = current_peers;
                retry_at = tokio::time::Instant::now() + Duration::from_secs(2);
            }

            // Try requesting when the retry timer fires
            if tokio::time::Instant::now() >= retry_at {
                retry_at = tokio::time::Instant::now() + Duration::from_secs(30); // don't retry again immediately
                if let Ok(rx) = node.request_content(&content_id) {
                    info!("Requesting content from provider...");
                    let mut rx = rx;
                    let req_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
                    loop {
                        if tokio::time::Instant::now() > req_deadline {
                            break;
                        }
                        tokio::select! {
                            result = &mut rx => {
                                if let Ok(Ok(resp)) = result {
                                    if !resp.data.is_empty() {
                                        response = resp;
                                    }
                                }
                                break;
                            }
                            event = node.next_event() => {
                                node.handle_swarm_event(event);
                            }
                        }
                    }
                    if !response.data.is_empty() {
                        break;
                    }
                }
            }
        }

        if response.data.is_empty() {
            anyhow::bail!("Content not found on any peer or via DHT");
        }
    }

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
