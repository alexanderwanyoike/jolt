use std::path::PathBuf;

use tokio::sync::{mpsc, oneshot};

use jolt_core::{
    EncryptedObjectRecipient, IdentityEncryptionKey, IdentityId, PinRequest, UpdateLogEntry,
};

use crate::command::{
    CacheEntryInfo, CacheStatsResponse, DaemonCommand, DecryptedObjectResponse,
    EncryptedObjectResponse, FetchResult, NodeStatus, PeerConnectResponse, PeerInfo,
    PublishResponse, PublishedContentInfo, ResolveResponse,
};
use crate::config::HomeRelayConfig;
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
    pub async fn publish(
        &self,
        file_path: PathBuf,
        path: Option<String>,
    ) -> Result<PublishResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Publish {
                file_path,
                path,
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

    /// Resolve a `.jolt` address to its current content target.
    pub async fn resolve(&self, address: String) -> Result<ResolveResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Resolve {
                address,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Ensure the daemon has a local encryption key for its identity.
    pub async fn ensure_local_identity_encryption_key(
        &self,
    ) -> Result<IdentityEncryptionKey, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::EnsureLocalIdentityEncryptionKey { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Encrypt plaintext into a signed Jolt encrypted object envelope.
    pub async fn encrypt_object(
        &self,
        plaintext: Vec<u8>,
        content_type: String,
        recipients: Vec<EncryptedObjectRecipient>,
    ) -> Result<EncryptedObjectResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::EncryptObject {
                plaintext,
                content_type,
                recipients,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Decrypt a signed Jolt encrypted object envelope for the local identity.
    pub async fn decrypt_object(
        &self,
        encrypted_object: Vec<u8>,
    ) -> Result<DecryptedObjectResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::DecryptObject {
                encrypted_object,
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

    /// List locally published content with path and relay pin state.
    pub async fn list_published_content(&self) -> Result<Vec<PublishedContentInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ListPublishedContent { response_tx: tx })
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

    /// Create an owner-signed request for a relay to pin locally published content.
    pub async fn create_pin_request(&self, content_id: String) -> Result<PinRequest, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::CreatePinRequest {
                content_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Record that the configured home relay accepted a pin for local content.
    pub async fn record_home_relay_pin(
        &self,
        content_id: String,
        path: Option<String>,
        relay: HomeRelayConfig,
        latest_sequence: u64,
    ) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::RecordHomeRelayPin {
                content_id,
                path,
                relay,
                latest_sequence,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Update runtime network settings after admin configuration changes.
    pub async fn update_network_settings(
        &self,
        configured_bootstrap_relays: Vec<String>,
        effective_bootstrap_relays: Vec<String>,
        home_relay: Option<HomeRelayConfig>,
    ) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::UpdateNetworkSettings {
                configured_bootstrap_relays,
                effective_bootstrap_relays,
                home_relay,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Pin an owner's signed update log and announce this node as its provider.
    pub async fn pin_update_log(&self, identity: IdentityId) -> Result<u64, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::PinUpdateLog {
                identity,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        rx.await
            .map_err(|_| NetworkError::Protocol("Daemon dropped response".to_string()))?
    }

    /// Store a verified owner update-log snapshot and announce this node as its provider.
    pub async fn store_update_log(
        &self,
        identity: IdentityId,
        entries: Vec<UpdateLogEntry>,
    ) -> Result<u64, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::StoreUpdateLog {
                identity,
                entries,
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
