use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::error::NetworkError;

/// Commands sent from HTTP handlers / CLI to the daemon event loop.
pub enum DaemonCommand {
    Publish {
        file_path: PathBuf,
        response_tx: oneshot::Sender<Result<PublishResponse, NetworkError>>,
    },
    Fetch {
        content_id: String,
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
    },
    GetStatus {
        response_tx: oneshot::Sender<NodeStatus>,
    },
    GetPeers {
        response_tx: oneshot::Sender<Vec<PeerInfo>>,
    },
    GetCacheStats {
        response_tx: oneshot::Sender<CacheStatsResponse>,
    },
    ListCacheEntries {
        response_tx: oneshot::Sender<Vec<CacheEntryInfo>>,
    },
    Pin {
        content_id: String,
        response_tx: oneshot::Sender<Result<(), NetworkError>>,
    },
    Unpin {
        content_id: String,
        response_tx: oneshot::Sender<Result<(), NetworkError>>,
    },
    Shutdown {
        response_tx: oneshot::Sender<()>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishResponse {
    pub content_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub data: Vec<u8>,
    pub content_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub peer_id: String,
    pub uptime_secs: u64,
    pub connected_peers: usize,
    pub published_count: usize,
    pub cached_count: usize,
    pub listen_addresses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatsResponse {
    pub total_cached: u64,
    pub total_published: u64,
    pub cached_items: usize,
    pub published_items: usize,
    pub pinned_items: usize,
    pub pinned_size: u64,
    pub max_size: u64,
    pub available: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryInfo {
    pub content_id: String,
    pub size: u64,
    pub cached_at: u64,
    pub last_accessed: u64,
    pub pinned: bool,
}
