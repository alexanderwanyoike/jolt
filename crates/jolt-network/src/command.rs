use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use jolt_core::{
    DeviceAuthorizationRecord, DeviceWriterLogEntry, EncryptedObjectRecipient,
    IdentityEncryptionKey, IdentityId, LiveReachabilityEndpoint, OfflineIngressEndpoint,
    PinRequest, RelayHint, RelayRecord, VerifiedReachability,
};

use crate::config::HomeRelayConfig;
use crate::error::NetworkError;

/// Commands sent from HTTP handlers / CLI to the daemon event loop.
pub enum DaemonCommand {
    Publish {
        file_path: PathBuf,
        path: Option<String>,
        response_tx: oneshot::Sender<Result<PublishResponse, NetworkError>>,
    },
    PublishAppend {
        file_path: PathBuf,
        path: String,
        response_tx: oneshot::Sender<Result<PublishResponse, NetworkError>>,
    },
    EnumerateAppendRecords {
        identity: IdentityId,
        path_prefix: String,
        response_tx: oneshot::Sender<Result<Vec<AppendRecordInfo>, NetworkError>>,
    },
    Fetch {
        content_id: String,
        response_tx: oneshot::Sender<Result<FetchResult, NetworkError>>,
    },
    Resolve {
        address: String,
        response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    },
    DiagnoseIdentity {
        identity: IdentityId,
        response_tx: oneshot::Sender<Result<RelayDiagnoseIdentityResponse, NetworkError>>,
    },
    EnsureLocalIdentityEncryptionKey {
        response_tx: oneshot::Sender<Result<IdentityEncryptionKey, NetworkError>>,
    },
    EncryptObject {
        plaintext: Vec<u8>,
        content_type: String,
        recipients: Vec<EncryptedObjectRecipient>,
        response_tx: oneshot::Sender<Result<EncryptedObjectResponse, NetworkError>>,
    },
    DecryptObject {
        encrypted_object: Vec<u8>,
        response_tx: oneshot::Sender<Result<DecryptedObjectResponse, NetworkError>>,
    },
    PublishReachability {
        sequence_hint: u64,
        expires_at: u64,
        live: Vec<LiveReachabilityEndpoint>,
        offline_ingress: Vec<OfflineIngressEndpoint>,
        response_tx: oneshot::Sender<Result<PublishReachabilityResponse, NetworkError>>,
    },
    SubmitIngress {
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
        response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
    },
    /// Deliver an ingress envelope to a remote recipient's daemon over p2p.
    /// Resolves and dials the peer, so unlike SubmitIngress this can fail on
    /// connectivity; the caller must treat anything but an Accepted response
    /// naming the intended recipient as a failed delivery.
    SendIngressToPeer {
        peer_id: String,
        receiver_id: String,
        encrypted_object: Vec<u8>,
        expires_at: Option<u64>,
        response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
    },
    ListPendingIngress {
        response_tx: oneshot::Sender<Result<Vec<IngressRecord>, NetworkError>>,
    },
    OpenIngress {
        ingress_id: String,
        response_tx: oneshot::Sender<Result<DecryptedObjectResponse, NetworkError>>,
    },
    AcceptIngress {
        ingress_id: String,
        response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
    },
    RejectIngress {
        ingress_id: String,
        response_tx: oneshot::Sender<Result<IngressRecord, NetworkError>>,
    },
    ConnectPeer {
        multiaddr: String,
        response_tx: oneshot::Sender<Result<PeerConnectResponse, NetworkError>>,
    },
    GetStatus {
        response_tx: oneshot::Sender<NodeStatus>,
    },
    RelayPinAllowed {
        owner: String,
        response_tx: oneshot::Sender<bool>,
    },
    SignLocalIdentity {
        payload: Vec<u8>,
        response_tx: oneshot::Sender<Vec<u8>>,
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
    ListPublishedContent {
        response_tx: oneshot::Sender<Vec<PublishedContentInfo>>,
    },
    InspectLocalRecord {
        path: String,
        response_tx: oneshot::Sender<Option<LocalRecordInfo>>,
    },
    Pin {
        content_id: String,
        response_tx: oneshot::Sender<Result<(), NetworkError>>,
    },
    CreatePinRequest {
        content_id: String,
        response_tx: oneshot::Sender<Result<PinRequest, NetworkError>>,
    },
    RecordHomeRelayPin {
        content_id: String,
        path: Option<String>,
        relay: HomeRelayConfig,
        latest_sequence: u64,
        response_tx: oneshot::Sender<Result<(), NetworkError>>,
    },
    UpdateNetworkSettings {
        configured_bootstrap_relays: Vec<String>,
        effective_bootstrap_relays: Vec<String>,
        home_relay: Option<HomeRelayConfig>,
        response_tx: oneshot::Sender<Result<(), NetworkError>>,
    },
    PinUpdateLog {
        identity: IdentityId,
        response_tx: oneshot::Sender<Result<u64, NetworkError>>,
    },
    StoreUpdateLog {
        identity: IdentityId,
        entries: Vec<jolt_core::UpdateLogEntry>,
        response_tx: oneshot::Sender<Result<u64, NetworkError>>,
    },
    StoreDeviceWriterLogs {
        identity: IdentityId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
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
pub struct AppendRecordInfo {
    pub path: String,
    pub content_id: String,
    pub device_id: String,
    pub device_sequence: u64,
    pub created_at: u64,
    pub entry_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub data: Vec<u8>,
    pub content_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalRecordInfo {
    pub path: String,
    pub content_id: String,
    /// Opaque revision token; equal if and only if it names the same winning log entry.
    pub revision: String,
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
pub struct RelayDiagnoseIdentityResponse {
    pub identity: String,
    pub provider_key: String,
    pub local_update_log_cache: RelayDiagnoseCacheObservation,
    pub identity_head_hint: RelayDiagnoseIdentityHeadObservation,
    pub local_provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
    pub provider_candidates: Vec<RelayDiagnoseProviderCandidate>,
    pub relay_forwarding: RelayDiagnoseForwarding,
    pub outcome: RelayDiagnoseOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDiagnoseCacheObservation {
    pub state: String,
    pub entry_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDiagnoseIdentityHeadObservation {
    pub state: String,
    pub fresh_count: usize,
    pub expired_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_peer_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelayDiagnoseProviderCandidate {
    pub peer_id: String,
    pub addrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDiagnoseForwarding {
    pub attempted: bool,
    pub target_count: usize,
    pub attempts: Vec<RelayDiagnoseForwardingAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDiagnoseForwardingAttempt {
    pub peer_id: String,
    pub status: String,
    pub candidate_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayDiagnoseOutcome {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedObjectResponse {
    pub data: Vec<u8>,
    pub size: u64,
    pub recipient_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptedObjectResponse {
    pub plaintext: Vec<u8>,
    pub size: u64,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishReachabilityResponse {
    pub identity: String,
    pub path: String,
    pub address: String,
    pub latest_sequence: u64,
    pub content_id: String,
    pub record: VerifiedReachability,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IngressStatus {
    Pending,
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressRecord {
    pub ingress_id: String,
    pub receiver_id: String,
    pub sender_identity: String,
    pub recipient_identity: String,
    pub schema_hint: Option<String>,
    pub status: IngressStatus,
    pub received_at: u64,
    pub expires_at: Option<u64>,
    pub size: u64,
    #[serde(skip)]
    pub encrypted_object: Vec<u8>,
    // default: these are omitted from the wire when None, so deserializing a
    // pending record (from the HTTP submit response or the p2p ingress-submit
    // response) must tolerate their absence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerConnectResponse {
    pub peer_id: Option<String>,
    pub multiaddr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatus {
    pub daemon_version: String,
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
    pub bootstrap_state: String,
    pub configured_bootstrap_relays: Vec<String>,
    pub configured_bootstrap_relay_count: usize,
    pub effective_bootstrap_relays: Vec<String>,
    pub effective_bootstrap_relay_count: usize,
    pub known_relay_count: usize,
    pub connected_bootstrap_peers: usize,
    pub last_bootstrap_error: Option<String>,
    pub home_relay: Option<HomeRelayConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_record: Option<RelayRecord>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedRelayInfo {
    pub peer_id: String,
    pub multiaddr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishedContentInfo {
    pub content_id: String,
    pub size: u64,
    pub path: Option<String>,
    pub address: Option<String>,
    pub local_sequence: Option<u64>,
    pub pin_state: String,
    pub relay: Option<PublishedRelayInfo>,
    pub pinned_content_id: Option<String>,
    pub pinned_sequence: Option<u64>,
}
