use std::{path::PathBuf, time::Duration};

use tokio::sync::{mpsc, oneshot};

use jolt_core::{
    DeviceAuthorizationRecord, DeviceWriterLogEntry, EncryptedObjectRecipient,
    IdentityEncryptionKey, IdentityId, LiveReachabilityEndpoint, OfflineIngressEndpoint,
    PinRequest, UpdateLogEntry,
};

use crate::command::{
    AppendRecordInfo, CacheEntryInfo, CacheStatsResponse, DaemonCommand, DecryptedObjectResponse,
    EncryptedObjectResponse, FetchResult, IngressRecord, NodeStatus, PeerConnectResponse, PeerInfo,
    PublishReachabilityResponse, PublishResponse, PublishedContentInfo,
    RelayDiagnoseIdentityResponse, ResolveResponse,
};
use crate::config::HomeRelayConfig;
use crate::error::NetworkError;

const DAEMON_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Timeout for commands whose responses are deferred until network operations
/// complete (DHT walks, peer dials, hole punching, content transfer). Must
/// exceed the longest underlying network budget: the fetch manager allows 60s
/// (`DEFAULT_FETCH_TIMEOUT`) and Kademlia queries allow 60s.
const DAEMON_NETWORK_COMMAND_TIMEOUT: Duration = Duration::from_secs(70);

async fn receive_result<T>(
    rx: oneshot::Receiver<Result<T, NetworkError>>,
) -> Result<T, NetworkError> {
    receive_result_with_timeout(rx, DAEMON_COMMAND_TIMEOUT).await
}

async fn receive_result_with_timeout<T>(
    rx: oneshot::Receiver<Result<T, NetworkError>>,
    timeout: Duration,
) -> Result<T, NetworkError> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(NetworkError::Protocol(
            "Daemon dropped response".to_string(),
        )),
        Err(_) => Err(NetworkError::Timeout),
    }
}

async fn receive_plain<T>(rx: oneshot::Receiver<T>) -> Result<T, NetworkError> {
    receive_plain_with_timeout(rx, DAEMON_COMMAND_TIMEOUT).await
}

async fn receive_plain_with_timeout<T>(
    rx: oneshot::Receiver<T>,
    timeout: Duration,
) -> Result<T, NetworkError> {
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err(NetworkError::Protocol(
            "Daemon dropped response".to_string(),
        )),
        Err(_) => Err(NetworkError::Timeout),
    }
}

/// Client-side handle to communicate with the daemon event loop.
///
/// This is `Send + Sync + Clone` and can be shared across HTTP handlers.
#[derive(Clone)]
pub struct DaemonHandle {
    cmd_tx: mpsc::Sender<DaemonCommand>,
    local_identity_address: Option<String>,
}

impl DaemonHandle {
    /// Create a new DaemonHandle from a command sender.
    pub fn new(cmd_tx: mpsc::Sender<DaemonCommand>) -> Self {
        Self {
            cmd_tx,
            local_identity_address: None,
        }
    }

    /// Create a new DaemonHandle with the local identity address available for
    /// cheap server-side authorization decisions.
    pub fn new_with_local_identity(
        cmd_tx: mpsc::Sender<DaemonCommand>,
        local_identity_address: String,
    ) -> Self {
        Self {
            cmd_tx,
            local_identity_address: Some(local_identity_address),
        }
    }

    /// Return the local identity address when it was captured at daemon startup.
    pub fn local_identity_address(&self) -> Option<&str> {
        self.local_identity_address.as_deref()
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
        receive_result(rx).await
    }

    /// Fetch content by ID. Returns the data. The response is deferred until
    /// the fetch manager finishes (up to 60s on real networks), so this waits
    /// with the network command timeout rather than the quick-command one.
    pub async fn fetch(&self, content_id: String) -> Result<FetchResult, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Fetch {
                content_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result_with_timeout(rx, DAEMON_NETWORK_COMMAND_TIMEOUT).await
    }

    /// Publish a file as an append record bound to a path. Returns the content
    /// ID and address. Append records coexist; this never rewrites a singleton.
    pub async fn publish_append(
        &self,
        file_path: PathBuf,
        path: String,
    ) -> Result<PublishResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::PublishAppend {
                file_path,
                path,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Enumerate an identity's cached append records whose path starts with the
    /// given prefix. This is how a Collection is read.
    pub async fn enumerate_append_records(
        &self,
        identity: IdentityId,
        path_prefix: String,
    ) -> Result<Vec<AppendRecordInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::EnumerateAppendRecords {
                identity,
                path_prefix,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Resolve a `.jolt` address to its current content target. Resolution may
    /// require DHT provider discovery, dialing candidates, and update-log or
    /// device-writer sync, so this waits with the network command timeout.
    pub async fn resolve(&self, address: String) -> Result<ResolveResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Resolve {
                address,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result_with_timeout(rx, DAEMON_NETWORK_COMMAND_TIMEOUT).await
    }

    /// Diagnose update-log provider discovery for a Jolt identity. The
    /// response is deferred until DHT probes finish, so this waits with the
    /// network command timeout.
    pub async fn diagnose_identity(
        &self,
        identity: IdentityId,
    ) -> Result<RelayDiagnoseIdentityResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::DiagnoseIdentity {
                identity,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result_with_timeout(rx, DAEMON_NETWORK_COMMAND_TIMEOUT).await
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
        receive_result(rx).await
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
        receive_result(rx).await
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
        receive_result(rx).await
    }

    /// Publish the local identity's signed reachability endpoint metadata.
    pub async fn publish_reachability(
        &self,
        sequence_hint: u64,
        expires_at: u64,
        live: Vec<LiveReachabilityEndpoint>,
        offline_ingress: Vec<OfflineIngressEndpoint>,
    ) -> Result<PublishReachabilityResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::PublishReachability {
                sequence_hint,
                expires_at,
                live,
                offline_ingress,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Submit a recipient-controlled ingress envelope to the local daemon.
    pub async fn submit_ingress(
        &self,
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
    ) -> Result<IngressRecord, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::SubmitIngress {
                receiver_id,
                encrypted_object,
                expires_at,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Deliver a recipient-controlled ingress envelope to a remote recipient's
    /// daemon over p2p. Fails when the peer cannot be reached or rejects the
    /// envelope; callers must also verify the returned record's
    /// `recipient_identity` names the identity they intended to reach.
    pub async fn send_ingress_to_peer(
        &self,
        peer_id: String,
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
    ) -> Result<IngressRecord, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::SendIngressToPeer {
                peer_id,
                receiver_id,
                encrypted_object,
                expires_at,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// List locally pending ingress envelopes.
    pub async fn list_pending_ingress(&self) -> Result<Vec<IngressRecord>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ListPendingIngress { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Decrypt a pending ingress envelope for local app review.
    pub async fn open_ingress(
        &self,
        ingress_id: String,
    ) -> Result<DecryptedObjectResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::OpenIngress {
                ingress_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Accept a pending ingress envelope.
    pub async fn accept_ingress(&self, ingress_id: String) -> Result<IngressRecord, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::AcceptIngress {
                ingress_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    /// Reject a pending ingress envelope.
    pub async fn reject_ingress(&self, ingress_id: String) -> Result<IngressRecord, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::RejectIngress {
                ingress_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
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
        receive_result(rx).await
    }

    /// Get the daemon's status.
    pub async fn status(&self) -> Result<NodeStatus, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetStatus { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }

    /// Ask the daemon's local identity key to sign protocol authority bytes.
    pub async fn sign_local_identity(&self, payload: Vec<u8>) -> Result<Vec<u8>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::SignLocalIdentity {
                payload,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }

    /// Get the list of connected peers.
    pub async fn peers(&self) -> Result<Vec<PeerInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetPeers { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> Result<CacheStatsResponse, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::GetCacheStats { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }

    /// List all cache entries.
    pub async fn list_cache_entries(&self) -> Result<Vec<CacheEntryInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ListCacheEntries { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }

    /// List locally published content with path and relay pin state.
    pub async fn list_published_content(&self) -> Result<Vec<PublishedContentInfo>, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ListPublishedContent { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
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
        receive_result(rx).await
    }

    /// Create an owner-signed request for a relay to pin locally published content.
    pub async fn create_pin_request(&self, content_id: String) -> Result<PinRequest, NetworkError> {
        self.create_pin_request_for_relay(content_id, true).await
    }

    /// Create a pin request matching the target relay's negotiated capabilities.
    pub async fn create_pin_request_for_relay(
        &self,
        content_id: String,
        include_declared_sizes: bool,
    ) -> Result<PinRequest, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::CreatePinRequest {
                content_id,
                include_declared_sizes,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    pub async fn reserve_relay_pin(
        &self,
        owner: String,
        request: crate::command::RelayPinRequestItems,
    ) -> Result<u64, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ReserveRelayPin {
                owner,
                request,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    pub async fn validate_relay_pin(
        &self,
        reservation_id: u64,
        items: Vec<crate::command::RelayPinItem>,
    ) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::ValidateRelayPin {
                reservation_id,
                items,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    pub async fn commit_relay_pin(
        &self,
        reservation_id: u64,
        items: Vec<crate::command::RelayPinItem>,
    ) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::CommitRelayPin {
                reservation_id,
                items,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
    }

    pub async fn cancel_relay_pin(&self, reservation_id: u64) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::CancelRelayPin {
                reservation_id,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
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
        receive_result(rx).await
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
        receive_result(rx).await
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
        receive_result(rx).await
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
        receive_result(rx).await
    }

    /// Store verified device-authority records plus per-device writer logs.
    pub async fn store_device_writer_logs(
        &self,
        identity: IdentityId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Result<u64, NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::StoreDeviceWriterLogs {
                identity,
                authority_records,
                device_logs,
                response_tx: tx,
            })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_result(rx).await
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
        receive_result(rx).await
    }

    /// Request graceful shutdown.
    pub async fn shutdown(&self) -> Result<(), NetworkError> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(DaemonCommand::Shutdown { response_tx: tx })
            .await
            .map_err(|_| NetworkError::Protocol("Daemon not running".to_string()))?;
        receive_plain(rx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_command_timeout_exceeds_fetch_manager_budget() {
        // A fetch's response arrives up to DEFAULT_FETCH_TIMEOUT after the
        // command is sent. Waiting less than that abandons operations the
        // network layer goes on to complete (issue #186).
        assert!(DAEMON_NETWORK_COMMAND_TIMEOUT > crate::fetch_manager::DEFAULT_FETCH_TIMEOUT);
    }

    #[tokio::test]
    async fn plain_daemon_response_times_out_when_loop_does_not_answer() {
        let (_tx, rx) = oneshot::channel::<()>();

        let result = receive_plain_with_timeout(rx, Duration::from_millis(1)).await;

        assert!(matches!(result, Err(NetworkError::Timeout)));
    }

    #[tokio::test]
    async fn result_daemon_response_times_out_when_loop_does_not_answer() {
        let (_tx, rx) = oneshot::channel::<Result<(), NetworkError>>();

        let result = receive_result_with_timeout(rx, Duration::from_millis(1)).await;

        assert!(matches!(result, Err(NetworkError::Timeout)));
    }
}
