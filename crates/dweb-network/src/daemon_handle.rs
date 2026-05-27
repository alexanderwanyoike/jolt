use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, FetchResult, NodeStatus,
    PeerConnectResponse, PeerInfo, PublishResponse,
};
use crate::error::NetworkError;

/// Client-side handle to communicate with the daemon event loop.
///
/// This is `Send + Sync + Clone` and can be shared across HTTP handlers.
#[derive(Clone)]
pub struct DaemonHandle {
    cmd_tx: mpsc::Sender<DaemonCommand>,
}

impl DaemonHandle {
    /// Create a new DaemonHandle from a command sender.
    pub fn new(cmd_tx: mpsc::Sender<DaemonCommand>) -> Self {
        Self { cmd_tx }
    }

    /// Publish a file. Returns the content ID and size.
    pub async fn publish(&self, file_path: PathBuf) -> Result<PublishResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Publish {
                file_path,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Fetch content by ID. Returns the data.
    pub async fn fetch(&self, content_id: String) -> Result<FetchResult, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Fetch {
                content_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Connect to a peer by multiaddr.
    pub async fn connect_peer(
        &self,
        multiaddr: String,
    ) -> Result<PeerConnectResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ConnectPeer {
                multiaddr,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Get the daemon's status.
    pub async fn status(&self) -> Result<NodeStatus, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetStatus { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))
    }

    /// Get the list of connected peers.
    pub async fn peers(&self) -> Result<Vec<PeerInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetPeers { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> Result<CacheStatsResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetCacheStats { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))
    }

    /// List all cache entries.
    pub async fn list_cache_entries(&self) -> Result<Vec<CacheEntryInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ListCacheEntries { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))
    }

    /// Pin content to prevent cache eviction.
    pub async fn pin(&self, content_id: String) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Pin {
                content_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Unpin content to allow cache eviction.
    pub async fn unpin(&self, content_id: String) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Unpin {
                content_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Request graceful shutdown.
    pub async fn shutdown(&self) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Shutdown { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))
    }
}
