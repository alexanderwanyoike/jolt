use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use dweb_core::{IdentityId, RelayHint};

use crate::config::HomeRelayConfig;
use crate::error::NetworkError;

/// Commands sent from HTTP handlers / CLI to the daemon event loop.
pub enum DaemonCommand {
    Publish {
        file_path: PathBuf,
        path: Option<String>,
        response_tx: oneshot::Sender<Result<PublishResponse, NetworkError>>,
    },
    Fetch {
        content_id: String,
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
    },
    Resolve {
        address: String,
        response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    },
    ConnectPeer {
        multiaddr: String,
        response_tx: oneshot::Sender<Result<PeerConnectResponse, NetworkError>>,
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
    PinUpdateLog {
        identity: IdentityId,
        response_tx: oneshot::Sender<Result<u64, NetworkError>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub data: Vec<u8>,
    pub content_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveResponse {
    pub address: String,
    pub identity: String,
    pub path: String,
    pub latest_sequence: u64,
    pub content_id: String,
    pub reachability_hints: Vec<RelayHint>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConnectResponse {
    pub peer_id: Option<String>,
    pub multiaddr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub peer_id: String,
    pub identity_address: String,
    pub uptime_secs: u64,
    pub connected_peers: usize,
    pub direct_peers: usize,
    pub relayed_peers: usize,
    pub nat_type: String,
    pub active_relays: usize,
    pub published_count: usize,
    pub cached_count: usize,
    pub listen_addresses: Vec<String>,
    pub bootstrap_relay: bool,
    pub configured_bootstrap_relays: Vec<String>,
    pub effective_bootstrap_relays: Vec<String>,
    pub home_relay: Option<HomeRelayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub is_relayed: bool,
    pub transport: String,
    pub remote_addr: String,
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
