use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use tracing::info;

use dweb_core::ContentId;
use dweb_identity::{verify_signature, NodeIdentity};
use dweb_network::NetworkNode;
use dweb_store::{CacheConfig, ContentStore};

use crate::config::NodeConfig;

pub async fn run(content_id_str: &str, output: Option<PathBuf>, dial: Option<String>) -> Result<()> {
    let content_id: ContentId = content_id_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid ContentId: {e}"))?;

    let config = NodeConfig::default_dirs();
    config.ensure_dirs()?;

    // Use a throwaway identity to avoid conflicting with a running `dweb start`,
    // but use the real content store so fetched content is cached persistently.
    let identity = NodeIdentity::generate();
    let store = ContentStore::open(&config.content_store_dir, CacheConfig::default())?;

    let mut node = NetworkNode::new(identity, store).await?;

    node.listen_on("/ip4/0.0.0.0/tcp/0")?;
    node.listen_on("/ip4/0.0.0.0/udp/0/quic-v1")?;

    info!("Fetching: {content_id}");

    // If a direct dial address is provided, connect to it
    if let Some(ref addr) = dial {
        info!("Dialing peer at {addr}");
        let multiaddr: dweb_network::Multiaddr = addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid multiaddr: {e}"))?;
        node.dial(multiaddr)?;
    } else {
        info!("Discovering peers via mDNS...");
    }

    // Wait for peer connection
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!(
                "Timed out waiting for peers. Is another dweb node running on this network?"
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

    // Request the content
    let rx = node.request_content(&content_id)?;

    // Pump events while waiting for response
    let response = {
        let mut rx = rx;
        let fetch_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if tokio::time::Instant::now() > fetch_deadline {
                anyhow::bail!("Timed out waiting for content");
            }

            tokio::select! {
                result = &mut rx => {
                    match result {
                        Ok(Ok(resp)) => break resp,
                        Ok(Err(e)) => anyhow::bail!("Fetch error: {e}"),
                        Err(_) => anyhow::bail!("Response channel closed"),
                    }
                }
                event = node.next_event() => {
                    node.handle_swarm_event(event);
                }
            }
        }
    };

    if response.data.is_empty() {
        anyhow::bail!("Content not found on any peer");
    }

    // Verify content
    if !content_id.verify(&response.data) {
        anyhow::bail!("Content verification failed: hash mismatch");
    }
    info!("Hash verified: content matches ContentId");

    // Verify signature
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

    // Save to file
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
