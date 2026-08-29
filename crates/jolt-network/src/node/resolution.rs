use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use jolt_core::{
    merge_device_writer_logs, resolve_jolt_address, resolve_merged_device_jolt_address,
    verify_identity_authority_chain, verify_update_log_for_identity, AuthorizedDeviceStatus,
    DeviceAuthorizationRecord, DeviceWriterLogEntry, DeviceWriterLogError, IdentityId, JoltAddress,
    ResolvedJoltTarget,
};
use jolt_store::{PersistedRemoteIdentityState, RemoteIdentityStateStore};
use tokio::sync::oneshot;

use crate::command::{
    AppendRecordInfo, MaterializedRecordInfo, MaterializedRecordRefreshOutcome,
    MaterializedRecordView, ResolveResponse,
};
use crate::error::NetworkError;
use crate::protocol::{
    DeviceWriterCursor, DeviceWriterSyncContinuation, DeviceWriterSyncRequest,
    DeviceWriterSyncResponse, UpdateLogRequest, DELTA_DEVICE_WRITER_SYNC_VERSION,
    LEGACY_DEVICE_WRITER_SYNC_VERSION,
};

use super::NetworkNode;

/// A waiter parked on a device-writer sync for a remote identity. Once the sync
/// completes (or fails), the waiter is answered from freshly merged state.
pub(super) enum DeviceWriterSyncWaiter {
    /// An `EnumerateAppendRecords` command waiting for live remote state.
    Enumerate {
        identity: IdentityId,
        path_prefix: String,
        response_tx: oneshot::Sender<Result<Vec<AppendRecordInfo>, NetworkError>>,
    },
    /// A bounded refresh returning the retained records plus freshness outcome.
    MaterializedView {
        identity: IdentityId,
        path_prefix: String,
        response_tx: oneshot::Sender<Result<MaterializedRecordView, NetworkError>>,
    },
    /// A background refresh that only populates the device-writer cache so that
    /// later enumerations/resolutions see live remote state. No waiter is
    /// answered. This is how a `.jolt` resolve opportunistically warms the
    /// device-writer cache while the legacy update-log path answers the resolve.
    Refresh { identity: IdentityId },
    /// A cooldown-bounded refresh for this installation's own identity. It is
    /// attempted only against an explicitly selected or mDNS-discovered peer
    /// and does not fan out to other providers after failure.
    LocalRefresh { identity: IdentityId },
}

impl DeviceWriterSyncWaiter {
    fn identity(&self) -> &IdentityId {
        match self {
            Self::Enumerate { identity, .. } | Self::MaterializedView { identity, .. } => identity,
            Self::Refresh { identity } | Self::LocalRefresh { identity } => identity,
        }
    }

    fn is_cancelled(&self) -> bool {
        match self {
            Self::Enumerate { response_tx, .. } => response_tx.is_closed(),
            Self::MaterializedView { response_tx, .. } => response_tx.is_closed(),
            Self::Refresh { .. } | Self::LocalRefresh { .. } => false,
        }
    }

    fn fail(self, error: NetworkError) {
        match self {
            Self::Enumerate { response_tx, .. } => {
                let _ = response_tx.send(Err(error));
            }
            Self::MaterializedView { response_tx, .. } => {
                let _ = response_tx.send(Err(error));
            }
            Self::Refresh { .. } | Self::LocalRefresh { .. } => {}
        }
    }
}

/// An in-flight device-writer sync request to a single provider.
pub(super) struct PendingDeviceWriterSync {
    pub(super) identity: IdentityId,
    pub(super) provider: libp2p::PeerId,
    pub(super) deadline: Instant,
    pub(super) requested_sync_version: u16,
    pub(super) requested_cursor_count: usize,
}

pub(super) struct DeviceWriterSyncWork {
    pub(super) id: u64,
    pub(super) identity: IdentityId,
    pub(super) provider: libp2p::PeerId,
    pub(super) deadline: Instant,
    pub(super) settled: bool,
    existing: Option<Arc<super::CachedDeviceWriterState>>,
    authority_records: Vec<DeviceAuthorizationRecord>,
    device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    sync_version: u16,
    heads: Vec<DeviceWriterCursor>,
    continuation: Option<DeviceWriterSyncContinuation>,
}

pub(super) struct ActiveDeviceWriterSyncWork {
    pub(super) identity: IdentityId,
    pub(super) provider: libp2p::PeerId,
    pub(super) deadline: Instant,
    pub(super) settled: bool,
    base_state: Option<Arc<super::CachedDeviceWriterState>>,
    continuation: Option<DeviceWriterSyncContinuation>,
}

pub(super) struct CompletedDeviceWriterSyncWork {
    pub(super) id: u64,
    pub(super) result: Result<super::CachedDeviceWriterState, NetworkError>,
}

pub(super) struct DeviceWriterSyncWorkQueue {
    pub(super) completion_tx: tokio::sync::mpsc::Sender<CompletedDeviceWriterSyncWork>,
    pub(super) completion_rx: tokio::sync::mpsc::Receiver<CompletedDeviceWriterSyncWork>,
    pub(super) queued: std::collections::VecDeque<DeviceWriterSyncWork>,
    pub(super) active: HashMap<u64, ActiveDeviceWriterSyncWork>,
    pub(super) by_identity: HashMap<IdentityId, u64>,
    pub(super) next_id: u64,
    pub(super) max_concurrency: usize,
    pub(super) queue_capacity: usize,
    pub(super) rejected: u64,
    pub(super) cancelled: u64,
    pub(super) timed_out: u64,
    pub(super) verified: u64,
    pub(super) verification_failed: u64,
    pub(super) full_responses: u64,
    pub(super) delta_responses: u64,
    pub(super) delta_continuations: u64,
    pub(super) received_entries: u64,
    pub(super) received_bytes: u64,
    pub(super) full_recoveries: u64,
}

pub(super) struct RemoteIdentityPersistenceWorker {
    shared: Arc<(Mutex<RemoteIdentityPersistenceState>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

struct RemoteIdentityPersistenceState {
    pending: HashMap<IdentityId, Arc<super::CachedDeviceWriterState>>,
    order: VecDeque<IdentityId>,
    active: bool,
    accepting: bool,
}

impl RemoteIdentityPersistenceWorker {
    pub(super) fn new(store: RemoteIdentityStateStore) -> Self {
        let shared = Arc::new((
            Mutex::new(RemoteIdentityPersistenceState {
                pending: HashMap::new(),
                order: VecDeque::new(),
                active: false,
                accepting: true,
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("jolt-remote-identity-persistence".to_string())
            .spawn(move || loop {
                let (identity, state) = {
                    let (lock, ready) = &*worker_shared;
                    let mut state = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    while state.order.is_empty() && state.accepting {
                        state = ready
                            .wait(state)
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                    }
                    let Some(identity) = state.order.pop_front() else {
                        break;
                    };
                    let snapshot = state
                        .pending
                        .remove(&identity)
                        .expect("queued persistence identity has a snapshot");
                    state.active = true;
                    (identity, snapshot)
                };

                let record = persisted_remote_identity_state(&identity, &state);
                if let Err(error) = store.save(&record) {
                    tracing::warn!(
                        %identity,
                        %error,
                        "verified remote identity state could not be persisted"
                    );
                }

                let (lock, ready) = &*worker_shared;
                let mut shared = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                shared.active = false;
                if shared.order.is_empty() {
                    ready.notify_all();
                }
            })
            .expect("remote identity persistence worker thread must start");
        Self {
            shared,
            thread: Some(thread),
        }
    }

    fn enqueue(
        &self,
        identity: IdentityId,
        state: Arc<super::CachedDeviceWriterState>,
    ) -> Result<(), NetworkError> {
        let (lock, ready) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if !shared.accepting {
            return Err(NetworkError::Protocol(
                "remote identity persistence worker is shutting down".to_string(),
            ));
        }
        if shared.pending.insert(identity.clone(), state).is_none() {
            shared.order.push_back(identity);
        }
        ready.notify_one();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn flush(&self) {
        let (lock, ready) = &*self.shared;
        let mut shared = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while shared.active || !shared.order.is_empty() {
            shared = ready
                .wait(shared)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    pub(super) fn shutdown(&mut self) {
        let (lock, ready) = &*self.shared;
        {
            let mut shared = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            shared.accepting = false;
            ready.notify_all();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for RemoteIdentityPersistenceWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl DeviceWriterSyncWorkQueue {
    pub(super) fn new(max_concurrency: usize, queue_capacity: usize) -> Self {
        let max_concurrency = max_concurrency.max(1);
        let (completion_tx, completion_rx) = tokio::sync::mpsc::channel(max_concurrency);
        Self {
            completion_tx,
            completion_rx,
            queued: std::collections::VecDeque::new(),
            active: HashMap::new(),
            by_identity: HashMap::new(),
            next_id: 0,
            max_concurrency,
            queue_capacity,
            rejected: 0,
            cancelled: 0,
            timed_out: 0,
            verified: 0,
            verification_failed: 0,
            full_responses: 0,
            delta_responses: 0,
            delta_continuations: 0,
            received_entries: 0,
            received_bytes: 0,
            full_recoveries: 0,
        }
    }
}

pub(super) struct PendingResolve {
    pub(super) address: JoltAddress,
    pub(super) response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
    pub(super) deadline: Instant,
    pub(super) fallback_response: Option<ResolveResponse>,
}

pub(super) struct PendingDaemonResolve {
    pub(super) address: JoltAddress,
    pub(super) now: Option<u64>,
    pub(super) provider: libp2p::PeerId,
    pub(super) response_tx: Option<oneshot::Sender<Result<ResolveResponse, NetworkError>>>,
    pub(super) deadline: Instant,
    pub(super) fallback_response: Option<ResolveResponse>,
}

impl NetworkNode {
    /// Resolve a Jolt address from this node's verified update-log cache.
    pub fn resolve_cached_jolt_address(
        &self,
        address: &JoltAddress,
        now: Option<u64>,
    ) -> Result<ResolvedJoltTarget, NetworkError> {
        let entries = self.update_logs.get(address.identity()).ok_or_else(|| {
            NetworkError::Protocol(format!(
                "No verified update log cached for {}",
                address.identity()
            ))
        })?;
        resolve_jolt_address(address, entries, now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))
    }

    pub(super) fn resolve_response_from_cache(
        &self,
        address: &JoltAddress,
        now: Option<u64>,
        source: impl Into<String>,
    ) -> Result<ResolveResponse, NetworkError> {
        let entries = self.update_logs.get(address.identity()).ok_or_else(|| {
            NetworkError::Protocol(format!(
                "No verified update log cached for {}",
                address.identity()
            ))
        })?;
        let latest_sequence = verify_update_log_for_identity(address.identity(), entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let target = resolve_jolt_address(address, entries, now)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        Ok(ResolveResponse {
            address: address.to_string(),
            identity: target.identity.to_string(),
            path: target.path,
            latest_sequence,
            content_id: target.content_id.to_string(),
            reachability_hints: target.reachability,
            source: source.into(),
        })
    }

    pub(super) fn resolve_device_writer_response_from_cache(
        &self,
        address: &JoltAddress,
        source: impl Into<String>,
    ) -> Result<ResolveResponse, NetworkError> {
        let state = self
            .device_writer_states
            .get(address.identity())
            .ok_or_else(|| {
                NetworkError::Protocol(format!(
                    "No verified device writer state cached for {}",
                    address.identity()
                ))
            })?;
        let target = resolve_merged_device_jolt_address(address, &state.merged).map_err(
            |error| match error {
                DeviceWriterLogError::PathTombstoned { path } => {
                    NetworkError::PathTombstoned { path }
                }
                error => NetworkError::Protocol(error.to_string()),
            },
        )?;

        Ok(ResolveResponse {
            address: address.to_string(),
            identity: target.identity.to_string(),
            path: target.path,
            latest_sequence: state.authority_sequence,
            content_id: target.content_id.to_string(),
            reachability_hints: target.reachability,
            source: source.into(),
        })
    }

    /// Enumerate the append records cached for `identity` whose path starts
    /// with `path_prefix`. This is the read seam a Collection is assembled
    /// from: it reads the merged device-writer state, never a rewritten blob.
    /// Returns an empty list when no device-writer state is cached for the
    /// identity.
    pub fn enumerate_append_records(
        &self,
        identity: &IdentityId,
        path_prefix: &str,
    ) -> Result<Vec<AppendRecordInfo>, NetworkError> {
        let Some(state) = self.device_writer_states.get(identity) else {
            return Ok(Vec::new());
        };
        Ok(state
            .merged
            .append_records_under(path_prefix)
            .into_iter()
            .map(|(path, entry)| AppendRecordInfo {
                path: path.to_string(),
                content_id: entry.content_id.to_string(),
                device_id: entry.device_id.clone(),
                device_sequence: entry.device_sequence,
                created_at: entry.created_at,
                entry_hash: entry.entry_hash.to_hex(),
            })
            .collect())
    }

    pub(super) fn materialized_record_view(
        &self,
        identity: &IdentityId,
        path_prefix: &str,
        refresh: MaterializedRecordRefreshOutcome,
    ) -> Result<MaterializedRecordView, NetworkError> {
        let records = self
            .device_writer_states
            .get(identity)
            .map(|state| {
                state
                    .merged
                    .singleton_paths
                    .iter()
                    .filter_map(|(path, entry)| {
                        let content_id = entry.content_id()?;
                        path.starts_with(path_prefix)
                            .then(|| MaterializedRecordInfo {
                                path: path.clone(),
                                content_id: content_id.to_string(),
                                revision: entry.entry_hash.to_hex(),
                                created_at: entry.created_at,
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let last_verified_at = self
            .device_writer_states
            .get(identity)
            .map(|state| state.last_verified_at)
            .filter(|verified_at| *verified_at > 0)
            .or_else(|| {
                (*identity == self.identity.identity_id()
                    && refresh == MaterializedRecordRefreshOutcome::Ready)
                    .then(super::unix_now)
            });
        Ok(MaterializedRecordView {
            records,
            last_verified_at,
            refresh,
        })
    }

    /// Store a verified merged device-writer state for an identity.
    ///
    /// Newly supplied authority records and per-device writer logs are merged
    /// with any already-cached state for the identity, so that device logs
    /// discovered from different providers (or in different orders) converge on
    /// the same deterministic merged view. The authority chain with the highest
    /// verified sequence wins, so a later sync that carries a device revocation
    /// is honoured.
    pub fn store_verified_device_writer_logs(
        &mut self,
        identity: IdentityId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Result<u64, NetworkError> {
        let existing = self.device_writer_states.get(&identity);
        let mut state = merge_verified_device_writer_state(
            &identity,
            existing.cloned(),
            authority_records,
            device_logs,
            LEGACY_DEVICE_WRITER_SYNC_VERSION,
            Vec::new(),
            true,
        )?;
        state.last_verified_at = super::unix_now();
        state.source_peer = None;
        if identity == self.identity.identity_id() {
            let local_device_id = self.local_device_id();
            if let Some(synced_local_log) = state.device_logs.get(&local_device_id) {
                let local_log = self
                    .local_device_writer_logs
                    .get(&identity)
                    .cloned()
                    .unwrap_or_default();
                let local_is_prefix = device_log_is_prefix(&local_log, synced_local_log);
                let synced_is_prefix = device_log_is_prefix(synced_local_log, &local_log);
                if !local_is_prefix && !synced_is_prefix {
                    self.blocked_local_device_writer_identities
                        .insert(identity.clone());
                    return Err(NetworkError::Protocol(format!(
                        "local device-writer history diverged for {identity}"
                    )));
                }
            }
        }
        let authority_sequence = state.authority_sequence;
        if identity != self.identity.identity_id() {
            self.persist_remote_identity_state(&identity, &state, None)?;
        }
        self.device_writer_states.insert(identity, Arc::new(state));
        Ok(authority_sequence)
    }

    fn persist_remote_identity_state(
        &self,
        identity: &IdentityId,
        state: &super::CachedDeviceWriterState,
        source_peer: Option<String>,
    ) -> Result<(), NetworkError> {
        let mut record = persisted_remote_identity_state(identity, state);
        record.source_peer = source_peer.or(record.source_peer);
        self.store
            .save_remote_identity_state(&record)
            .map_err(|error| {
                NetworkError::Protocol(format!(
                    "failed to persist verified remote identity state for {identity}: {error}"
                ))
            })
    }

    pub(super) fn restore_persisted_remote_identity_state(
        &mut self,
        record: PersistedRemoteIdentityState,
    ) -> Result<(), NetworkError> {
        let identity = record.identity;
        if identity == self.identity.identity_id() {
            return Err(NetworkError::Protocol(
                "remote identity state cannot replace local-authority state".to_string(),
            ));
        }
        let mut state = merge_verified_device_writer_state(
            &identity,
            None,
            record.authority_records,
            record.device_logs,
            LEGACY_DEVICE_WRITER_SYNC_VERSION,
            Vec::new(),
            true,
        )?;
        state.last_verified_at = record.last_verified_at;
        state.source_peer = record.source_peer;
        self.device_writer_states.insert(identity, Arc::new(state));
        Ok(())
    }

    /// A provider-facing snapshot of the device-writer state cached for an
    /// identity: the verified authority chain plus every per-device writer log
    /// this node can serve. Returns `None` when no state is cached.
    pub(super) fn device_writer_sync_snapshot(
        &self,
        identity: &IdentityId,
    ) -> Option<(
        Vec<DeviceAuthorizationRecord>,
        Vec<Vec<DeviceWriterLogEntry>>,
    )> {
        let state = self.device_writer_states.get(identity)?;
        let mut device_logs: Vec<_> = state.device_logs.values().cloned().collect();
        // Serve in a deterministic order so responses are reproducible.
        device_logs.sort_by(|left, right| {
            let left_id = left
                .first()
                .map(|e| e.body.device_id.as_str())
                .unwrap_or("");
            let right_id = right
                .first()
                .map(|e| e.body.device_id.as_str())
                .unwrap_or("");
            left_id.cmp(right_id)
        });
        Some((state.authority_records.clone(), device_logs))
    }

    pub(super) fn device_writer_sync_cursors(
        &self,
        identity: &IdentityId,
    ) -> Vec<DeviceWriterCursor> {
        let Some(state) = self.device_writer_states.get(identity) else {
            return Vec::new();
        };
        let mut cursors: Vec<_> = state
            .device_logs
            .values()
            .filter_map(|log| log.last().map(DeviceWriterCursor::from_entry))
            .collect();
        cursors.sort_by(|left, right| left.device_id.cmp(&right.device_id));
        cursors
    }

    pub(super) fn request_daemon_resolve_from_provider(
        &mut self,
        address: JoltAddress,
        now: Option<u64>,
        provider: &libp2p::PeerId,
        response_tx: oneshot::Sender<Result<ResolveResponse, NetworkError>>,
        fallback_response: Option<ResolveResponse>,
    ) {
        self.request_daemon_update_log_from_provider(
            address,
            now,
            provider,
            Some(response_tx),
            fallback_response,
        );
    }

    pub(super) fn request_daemon_refresh_from_provider(
        &mut self,
        address: JoltAddress,
        provider: &libp2p::PeerId,
    ) {
        self.request_daemon_update_log_from_provider(address, None, provider, None, None);
    }

    pub(super) fn request_daemon_update_log_from_provider(
        &mut self,
        address: JoltAddress,
        now: Option<u64>,
        provider: &libp2p::PeerId,
        response_tx: Option<oneshot::Sender<Result<ResolveResponse, NetworkError>>>,
        fallback_response: Option<ResolveResponse>,
    ) {
        let request = UpdateLogRequest {
            identity: address.identity().clone(),
            since: self
                .update_logs
                .get(address.identity())
                .and_then(|entries| {
                    verify_update_log_for_identity(address.identity(), entries).ok()
                }),
        };

        let request_id = self
            .swarm
            .behaviour_mut()
            .update_log_sync
            .send_request(provider, request);
        self.pending_daemon_resolutions.insert(
            request_id,
            PendingDaemonResolve {
                address,
                now,
                provider: *provider,
                response_tx,
                deadline: Instant::now() + self.resolve_timeout,
                fallback_response,
            },
        );
    }

    /// Whether this node currently caches device-writer state for an identity.
    pub(super) fn has_device_writer_state(&self, identity: &IdentityId) -> bool {
        self.device_writer_states.contains_key(identity)
    }

    /// Begin a device-writer sync for a remote identity on behalf of `waiter`.
    ///
    /// This mirrors the legacy update-log resolve path: reuse provider discovery
    /// (the same `jolt:update-log:<identity>` DHT/relay key), then request the
    /// provider's device-authority records and per-device writer logs over the
    /// dedicated device-writer sync protocol. The waiter is answered once the
    /// response is verified and merged, or when discovery/sync gives up.
    pub(super) fn begin_device_writer_sync(&mut self, waiter: DeviceWriterSyncWaiter) {
        let identity = waiter.identity().clone();
        let is_background_refresh = matches!(
            &waiter,
            DeviceWriterSyncWaiter::Refresh { .. } | DeviceWriterSyncWaiter::LocalRefresh { .. }
        );

        // Explicit enumerations wait for an in-flight sync. Background refresh
        // waiters carry no response, so accumulating one per cache hit only
        // wastes memory and work.
        let sync_in_flight = self
            .pending_device_writer_syncs
            .values()
            .any(|pending| pending.identity == identity)
            || self
                .device_writer_sync_work
                .by_identity
                .contains_key(&identity);
        if sync_in_flight {
            if is_background_refresh {
                return;
            }
            self.pending_device_writer_waiters
                .entry(identity)
                .or_default()
                .push(waiter);
            return;
        }

        // Peek (do not consume) so the legacy update-log resolve path can still
        // take the same provider; both paths share the provider pool.
        let provider = self.peek_discovered_update_log_provider_except(&identity, None);

        // If there is neither a known provider nor any realistic way to discover
        // one (no relays, no connected peers), do not park the waiter forever:
        // answer it immediately from whatever is cached. For an unknown remote
        // identity this is an empty enumeration, which is a valid answer.
        if provider.is_none() && !self.should_refresh_cached_resolution(&identity) {
            self.pending_device_writer_waiters
                .entry(identity.clone())
                .or_default()
                .push(waiter);
            self.answer_device_writer_waiters_with_outcome(
                &identity,
                MaterializedRecordRefreshOutcome::NetworkUnavailable,
            );
            return;
        }

        if is_background_refresh && !self.mark_device_writer_refresh_if_due(&identity) {
            return;
        }

        self.find_update_log_providers(&identity);
        let provider =
            provider.or_else(|| self.peek_discovered_update_log_provider_except(&identity, None));
        self.pending_device_writer_waiters
            .entry(identity.clone())
            .or_default()
            .push(waiter);
        if let Some(provider) = provider {
            self.request_device_writer_sync_from_provider(identity, &provider);
        }
    }

    /// Refresh a cached legacy update log without making each path resolution
    /// perform its own provider lookup and sync. The cached response has already
    /// been returned to the caller when this method runs.
    pub(super) fn begin_cached_update_log_refresh(&mut self, address: JoltAddress) {
        let identity = address.identity().clone();
        if !self.should_refresh_cached_resolution(&identity)
            || !Self::mark_refresh_if_due(&mut self.cached_update_log_refreshes, &identity)
        {
            return;
        }

        self.find_update_log_providers(&identity);
        if let Some(provider) = self.take_discovered_update_log_provider(&identity) {
            self.request_daemon_refresh_from_provider(address, &provider);
        }
    }

    fn mark_device_writer_refresh_if_due(&mut self, identity: &IdentityId) -> bool {
        Self::mark_refresh_if_due(&mut self.device_writer_refreshes, identity)
    }

    fn mark_refresh_if_due(
        refreshes: &mut HashMap<IdentityId, Instant>,
        identity: &IdentityId,
    ) -> bool {
        let now = Instant::now();
        refreshes.retain(|_, refreshed_at| {
            now.saturating_duration_since(*refreshed_at) < super::CACHED_IDENTITY_REFRESH_INTERVAL
        });
        if refreshes.contains_key(identity) {
            return false;
        }
        refreshes.insert(identity.clone(), now);
        true
    }

    pub(super) fn request_device_writer_sync_from_provider(
        &mut self,
        identity: IdentityId,
        provider: &libp2p::PeerId,
    ) {
        let request = if identity == self.identity.identity_id() {
            self.device_writer_sync_snapshot(&identity)
                .map(|(authority_records, device_logs)| {
                    DeviceWriterSyncRequest::offering(
                        identity.clone(),
                        authority_records,
                        device_logs,
                    )
                })
                .unwrap_or_else(|| DeviceWriterSyncRequest::new(identity.clone()))
        } else {
            DeviceWriterSyncRequest::new(identity.clone())
                .with_cursors(self.device_writer_sync_cursors(&identity))
        };
        self.send_device_writer_sync_request(
            identity,
            provider,
            request,
            Instant::now() + self.resolve_timeout,
        );
    }

    fn request_device_writer_sync_continuation(
        &mut self,
        identity: IdentityId,
        provider: &libp2p::PeerId,
        continuation: DeviceWriterSyncContinuation,
        deadline: Instant,
    ) {
        let request =
            DeviceWriterSyncRequest::new(identity.clone()).with_cursors(continuation.cursors);
        self.send_device_writer_sync_request(identity, provider, request, deadline);
    }

    fn send_device_writer_sync_request(
        &mut self,
        identity: IdentityId,
        provider: &libp2p::PeerId,
        request: DeviceWriterSyncRequest,
        deadline: Instant,
    ) {
        let requested_sync_version = request.max_sync_version;
        let requested_cursor_count = request.cursors.len();
        let request_id = self
            .swarm
            .behaviour_mut()
            .device_writer_sync
            .send_request(provider, request);
        self.pending_device_writer_syncs.insert(
            request_id,
            PendingDeviceWriterSync {
                identity,
                provider: *provider,
                deadline,
                requested_sync_version,
                requested_cursor_count,
            },
        );
    }

    pub(super) fn refresh_local_device_writer_state_from_candidate(
        &mut self,
        peer_id: libp2p::PeerId,
        explicit: bool,
    ) {
        let identity = self.identity.identity_id();
        if !self.has_device_writer_state(&identity) {
            return;
        }
        let key = Self::update_log_provider_key(&identity);
        let providers = self.discovered_providers.entry(key).or_default();
        providers.retain(|provider| provider != &peer_id);
        providers.insert(0, peer_id);
        let already_in_flight = self
            .pending_device_writer_syncs
            .values()
            .any(|pending| pending.identity == identity);
        if already_in_flight {
            return;
        }
        if explicit {
            self.device_writer_refreshes
                .insert(identity.clone(), Instant::now());
        } else if !self.mark_device_writer_refresh_if_due(&identity) {
            return;
        }
        self.pending_device_writer_waiters
            .entry(identity.clone())
            .or_default()
            .push(DeviceWriterSyncWaiter::LocalRefresh {
                identity: identity.clone(),
            });
        self.request_device_writer_sync_from_provider(identity, &peer_id);
    }

    pub(super) fn refresh_local_device_writer_state_from_connected_peer(&mut self) {
        self.pending_local_device_writer_refresh = true;
        self.retry_pending_local_device_writer_refresh();
    }

    pub(super) fn retry_pending_local_device_writer_refresh(&mut self) {
        let identity = self.identity.identity_id();
        if self
            .pending_device_writer_syncs
            .values()
            .any(|pending| pending.identity == identity)
        {
            return;
        }

        if self.pending_local_device_writer_refresh_peers.is_empty() {
            if !self.pending_local_device_writer_refresh {
                return;
            }
            self.pending_local_device_writer_refresh = false;
            let mut peers: Vec<_> = self
                .swarm
                .connected_peers()
                .filter(|peer| {
                    self.verified_local_device_sync_peers.contains(peer)
                        && self.local_authority_authorizes_peer(peer)
                })
                .copied()
                .collect();
            peers.sort();
            self.pending_local_device_writer_refresh_peers = peers.into();
        }

        if let Some(provider) = self.pending_local_device_writer_refresh_peers.pop_front() {
            // A completed local mutation carries new signed history, so it must
            // not be suppressed by the connection-refresh cooldown.
            self.refresh_local_device_writer_state_from_candidate(provider, true);
        }
    }

    pub(super) fn local_authority_authorizes_peer(&self, peer: &libp2p::PeerId) -> bool {
        let identity = self.identity.identity_id();
        self.device_writer_states
            .get(&identity)
            .is_some_and(|state| {
                authority_records_authorize_peer(&identity, &state.authority_records, peer)
            })
    }

    /// Dispatch parked device-writer sync waiters to a freshly discovered
    /// provider for `identity`.
    pub(super) fn request_pending_device_writer_syncs_from_provider(
        &mut self,
        identity: &IdentityId,
        provider: &libp2p::PeerId,
    ) {
        if !self.pending_device_writer_waiters.contains_key(identity) {
            return;
        }
        let already_in_flight = self
            .pending_device_writer_syncs
            .values()
            .any(|pending| &pending.identity == identity);
        if already_in_flight {
            return;
        }
        self.request_device_writer_sync_from_provider(identity.clone(), provider);
    }

    /// Answer every parked waiter for an identity from current cached state,
    /// then clear them. Enumerate waiters always succeed with whatever is cached
    /// (an empty list is a valid answer when no remote state could be synced);
    /// refresh waiters carry no response and simply warm the cache.
    pub(super) fn answer_device_writer_waiters(&mut self, identity: &IdentityId) {
        self.answer_device_writer_waiters_with_outcome(
            identity,
            MaterializedRecordRefreshOutcome::Ready,
        );
    }

    pub(super) fn answer_device_writer_waiters_with_outcome(
        &mut self,
        identity: &IdentityId,
        refresh: MaterializedRecordRefreshOutcome,
    ) {
        let Some(waiters) = self.pending_device_writer_waiters.remove(identity) else {
            return;
        };
        for waiter in waiters {
            match waiter {
                DeviceWriterSyncWaiter::Enumerate {
                    identity,
                    path_prefix,
                    response_tx,
                } => {
                    let _ =
                        response_tx.send(self.enumerate_append_records(&identity, &path_prefix));
                }
                DeviceWriterSyncWaiter::MaterializedView {
                    identity,
                    path_prefix,
                    response_tx,
                } => {
                    let _ = response_tx.send(self.materialized_record_view(
                        &identity,
                        &path_prefix,
                        refresh,
                    ));
                }
                DeviceWriterSyncWaiter::Refresh { .. }
                | DeviceWriterSyncWaiter::LocalRefresh { .. } => {}
            }
        }
    }

    pub(super) fn check_device_writer_sync_timeouts(&mut self) {
        let now = Instant::now();
        let timed_out: Vec<_> = self
            .pending_device_writer_syncs
            .iter()
            .filter_map(|(request_id, pending)| (pending.deadline <= now).then_some(*request_id))
            .collect();

        for request_id in timed_out {
            if let Some(pending) = self.pending_device_writer_syncs.remove(&request_id) {
                self.on_device_writer_sync_settled(
                    &pending.identity,
                    &pending.provider,
                    Some(NetworkError::Timeout),
                );
                self.retry_pending_local_device_writer_refresh();
            }
        }
        self.expire_device_writer_sync_work(now);
        self.prune_cancelled_device_writer_sync_waiters();
    }

    fn expire_device_writer_sync_work(&mut self, now: Instant) {
        let expired_active: Vec<_> = self
            .device_writer_sync_work
            .active
            .iter()
            .filter_map(|(id, work)| (!work.settled && work.deadline <= now).then_some(*id))
            .collect();
        for id in expired_active {
            let Some(work) = self.device_writer_sync_work.active.get_mut(&id) else {
                continue;
            };
            work.settled = true;
            let identity = work.identity.clone();
            self.device_writer_sync_work.timed_out += 1;
            self.answer_device_writer_waiters_with_outcome(
                &identity,
                MaterializedRecordRefreshOutcome::NetworkUnavailable,
            );
        }

        let mut expired_queued = Vec::new();
        for work in &mut self.device_writer_sync_work.queued {
            if !work.settled && work.deadline <= now {
                work.settled = true;
                expired_queued.push(work.identity.clone());
            }
        }
        for identity in expired_queued {
            self.device_writer_sync_work.timed_out += 1;
            self.answer_device_writer_waiters_with_outcome(
                &identity,
                MaterializedRecordRefreshOutcome::NetworkUnavailable,
            );
        }
    }

    fn prune_cancelled_device_writer_sync_waiters(&mut self) {
        let mut cancelled_identities = Vec::new();
        let mut cancelled_count = 0_u64;
        self.pending_device_writer_waiters
            .retain(|identity, waiters| {
                let before = waiters.len();
                waiters.retain(|waiter| !waiter.is_cancelled());
                cancelled_count += (before - waiters.len()) as u64;
                if waiters.is_empty() {
                    cancelled_identities.push(identity.clone());
                    false
                } else {
                    true
                }
            });
        self.device_writer_sync_work.cancelled += cancelled_count;
        for identity in cancelled_identities {
            self.cancel_device_writer_sync_for_identity(&identity);
        }
    }

    fn cancel_device_writer_sync_for_identity(&mut self, identity: &IdentityId) {
        self.pending_device_writer_syncs
            .retain(|_, pending| &pending.identity != identity);
        let Some(id) = self.device_writer_sync_work.by_identity.remove(identity) else {
            return;
        };
        self.device_writer_sync_work
            .queued
            .retain(|work| work.id != id);
        if let Some(active) = self.device_writer_sync_work.active.get_mut(&id) {
            active.settled = true;
        }
    }

    pub(super) fn shutdown_device_writer_sync_work(&mut self) {
        for (_, waiters) in self.pending_device_writer_waiters.drain() {
            for waiter in waiters {
                waiter.fail(NetworkError::ShuttingDown);
            }
        }
        self.pending_device_writer_syncs.clear();
        self.device_writer_sync_work.queued.clear();
        self.device_writer_sync_work.by_identity.clear();
        for active in self.device_writer_sync_work.active.values_mut() {
            active.settled = true;
        }
    }

    /// Drive a device-writer sync forward after a response, failure, or timeout
    /// from `provider`. On success the state is already merged; on failure this
    /// retries the next discovered provider, and only answers parked waiters
    /// once no providers remain.
    pub(super) fn on_device_writer_sync_settled(
        &mut self,
        identity: &IdentityId,
        provider: &libp2p::PeerId,
        error: Option<NetworkError>,
    ) {
        if error.is_none() {
            self.answer_device_writer_waiters(identity);
            return;
        }
        let refresh_outcome = match error.as_ref() {
            Some(NetworkError::VerificationFailed)
            | Some(NetworkError::DiscoveryFailed {
                code: crate::error::DiscoveryFailureCode::IdentityHeadInvalid,
                ..
            })
            | Some(NetworkError::UnsupportedDeviceWriterOperationVersion { .. })
            | Some(NetworkError::UnsupportedDeviceWriterSyncVersion { .. }) => {
                MaterializedRecordRefreshOutcome::VerificationFailed
            }
            _ => MaterializedRecordRefreshOutcome::NetworkUnavailable,
        };

        let local_refresh_only = self
            .pending_device_writer_waiters
            .get(identity)
            .is_some_and(|waiters| {
                !waiters.is_empty()
                    && waiters
                        .iter()
                        .all(|waiter| matches!(waiter, DeviceWriterSyncWaiter::LocalRefresh { .. }))
            });
        if local_refresh_only {
            self.answer_device_writer_waiters(identity);
            return;
        }

        // Retry the next discovered provider. Peek (do not consume) so the
        // shared provider pool stays intact for the legacy update-log path; the
        // sync-timeout safety net guarantees parked waiters are eventually
        // answered even if every provider is unreachable.
        if let Some(next_provider) =
            self.peek_discovered_update_log_provider_except(identity, Some(provider))
        {
            self.request_device_writer_sync_from_provider(identity.clone(), &next_provider);
            return;
        }

        // No more providers: answer waiters from whatever is cached (which may
        // be nothing for a never-seen identity).
        self.answer_device_writer_waiters_with_outcome(identity, refresh_outcome);
    }

    pub(super) fn should_refresh_cached_resolution(&self, identity: &IdentityId) -> bool {
        let key = Self::update_log_provider_key(identity);
        self.discovered_providers
            .get(&key)
            .is_some_and(|providers| !providers.is_empty())
            || self.swarm.connected_peers().next().is_some()
            || !self.effective_bootstrap_relays.is_empty()
    }

    pub(super) fn request_pending_resolves_from_provider(
        &mut self,
        identity: &IdentityId,
        provider: &libp2p::PeerId,
    ) {
        let Some(pending) = self.pending_resolves.remove(identity) else {
            return;
        };

        for pending in pending {
            self.request_daemon_resolve_from_provider(
                pending.address,
                None,
                provider,
                pending.response_tx,
                pending.fallback_response,
            );
        }
    }

    pub(super) fn check_resolve_timeouts(&mut self) {
        let now = Instant::now();
        let mut empty = Vec::new();
        let no_bootstrap_relays = self.effective_bootstrap_relays.is_empty()
            && self.swarm.connected_peers().next().is_none();
        let relay_unreachable = !self.effective_bootstrap_relays.is_empty()
            && self.connected_bootstrap_peer_count() == 0;

        for (identity, pending) in &mut self.pending_resolves {
            let mut still_waiting = Vec::new();
            for pending in pending.drain(..) {
                if pending.deadline <= now {
                    let result = pending.fallback_response.map(Ok).unwrap_or_else(|| {
                        Err(Self::identity_provider_failure(
                            identity,
                            no_bootstrap_relays,
                            relay_unreachable,
                        ))
                    });
                    let _ = pending.response_tx.send(result);
                } else {
                    still_waiting.push(pending);
                }
            }
            *pending = still_waiting;
            if pending.is_empty() {
                empty.push(identity.clone());
            }
        }

        for identity in empty {
            self.pending_resolves.remove(&identity);
        }

        let timed_out: Vec<_> = self
            .pending_daemon_resolutions
            .iter()
            .filter_map(|(request_id, pending)| (pending.deadline <= now).then_some(*request_id))
            .collect();

        for request_id in timed_out {
            if let Some(pending) = self.pending_daemon_resolutions.remove(&request_id) {
                if let Some(response_tx) = pending.response_tx {
                    let result = pending
                        .fallback_response
                        .map(Ok)
                        .unwrap_or(Err(NetworkError::Timeout));
                    let _ = response_tx.send(result);
                }
            }
        }
    }

    /// Set the timeout for daemon `.jolt` provider discovery.
    pub fn set_resolve_timeout(&mut self, timeout: Duration) {
        self.resolve_timeout = timeout;
    }
}

fn persisted_remote_identity_state(
    identity: &IdentityId,
    state: &super::CachedDeviceWriterState,
) -> PersistedRemoteIdentityState {
    let mut device_logs: Vec<_> = state.device_logs.values().cloned().collect();
    device_logs.sort_by(|left, right| {
        left.first()
            .map(|entry| entry.body.device_id.as_str())
            .unwrap_or("")
            .cmp(
                right
                    .first()
                    .map(|entry| entry.body.device_id.as_str())
                    .unwrap_or(""),
            )
    });
    PersistedRemoteIdentityState {
        version: PersistedRemoteIdentityState::VERSION,
        identity: identity.clone(),
        authority_records: state.authority_records.clone(),
        device_logs,
        last_verified_at: state.last_verified_at,
        source_peer: state.source_peer.clone(),
    }
}

pub(super) fn device_log_is_prefix(
    prefix: &[DeviceWriterLogEntry],
    candidate: &[DeviceWriterLogEntry],
) -> bool {
    prefix.len() <= candidate.len()
        && prefix
            .iter()
            .zip(candidate)
            .all(|(left, right)| left.entry_hash() == right.entry_hash())
}

pub(super) fn authority_records_authorize_peer(
    identity: &IdentityId,
    authority_records: &[DeviceAuthorizationRecord],
    peer: &libp2p::PeerId,
) -> bool {
    let Ok(authority) = verify_identity_authority_chain(identity, authority_records) else {
        return false;
    };
    authority.devices.values().any(|device| {
        if device.status != AuthorizedDeviceStatus::Active {
            return false;
        }
        libp2p::identity::ed25519::PublicKey::try_from_bytes(&device.signing_public_key)
            .map(libp2p::identity::PublicKey::from)
            .is_ok_and(|public_key| public_key.to_peer_id() == *peer)
    })
}

fn device_log_is_newer(candidate: &[DeviceWriterLogEntry], known: &[DeviceWriterLogEntry]) -> bool {
    match candidate.len().cmp(&known.len()) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            let candidate = candidate
                .last()
                .expect("empty device logs are filtered before comparison");
            let known = known.last().expect("stored device logs are never empty");
            (
                candidate.body.created_at,
                candidate.body.device_sequence,
                &candidate.body.device_id,
                candidate.entry_hash().0,
            ) > (
                known.body.created_at,
                known.body.device_sequence,
                &known.body.device_id,
                known.entry_hash().0,
            )
        }
    }
}

fn merge_verified_device_writer_state(
    identity: &IdentityId,
    existing: Option<Arc<super::CachedDeviceWriterState>>,
    authority_records: Vec<DeviceAuthorizationRecord>,
    device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    sync_version: u16,
    heads: Vec<DeviceWriterCursor>,
    complete: bool,
) -> Result<super::CachedDeviceWriterState, NetworkError> {
    let last_verified_at = existing
        .as_ref()
        .map(|state| state.last_verified_at)
        .unwrap_or_default();
    let source_peer = existing
        .as_ref()
        .and_then(|state| state.source_peer.clone());
    let candidate_authority = verify_identity_authority_chain(identity, &authority_records)
        .map_err(|error| NetworkError::Protocol(error.to_string()))?;
    let use_candidate_authority = existing
        .as_ref()
        .map(|state| candidate_authority.latest_sequence >= state.authority_sequence)
        .unwrap_or(true);
    let authority_records = if use_candidate_authority {
        authority_records
    } else {
        existing
            .as_ref()
            .map(|state| state.authority_records.clone())
            .unwrap_or(authority_records)
    };
    let authority = verify_identity_authority_chain(identity, &authority_records)
        .map_err(|error| NetworkError::Protocol(error.to_string()))?;

    let mut accumulated: HashMap<String, Vec<DeviceWriterLogEntry>> = existing
        .map(|state| state.device_logs.clone())
        .unwrap_or_default();
    for log in device_logs {
        let Some(first) = log.first() else {
            continue;
        };
        let device_id = first.body.device_id.clone();
        let log = if first.body.device_sequence == 0 {
            log
        } else {
            let Some(known) = accumulated.get(&device_id) else {
                return Err(NetworkError::Protocol(format!(
                    "device-writer delta for {device_id} has no verified predecessor"
                )));
            };
            let known_head = known
                .last()
                .expect("stored device-writer logs are never empty");
            if first.body.device_sequence != known_head.body.device_sequence + 1
                || first.body.previous_entry_hash.as_ref() != Some(&known_head.entry_hash())
            {
                return Err(NetworkError::Protocol(format!(
                    "device-writer delta for {device_id} is not contiguous with verified state"
                )));
            }
            let mut extended = known.clone();
            extended.extend(log);
            extended
        };
        let keep = accumulated
            .get(&device_id)
            .map(|known| device_log_is_newer(&log, known))
            .unwrap_or(true);
        if keep {
            accumulated.insert(device_id, log);
        }
    }

    if sync_version >= DELTA_DEVICE_WRITER_SYNC_VERSION && complete {
        let heads_by_device: HashMap<_, _> = heads
            .iter()
            .map(|head| (head.device_id.as_str(), head))
            .collect();
        if heads_by_device.len() != heads.len() || heads_by_device.len() != accumulated.len() {
            return Err(NetworkError::Protocol(
                "device-writer delta heads do not describe complete provider state".to_string(),
            ));
        }
        for (device_id, log) in &accumulated {
            let Some(expected) = heads_by_device.get(device_id.as_str()) else {
                return Err(NetworkError::Protocol(format!(
                    "device-writer delta omitted head for {device_id}"
                )));
            };
            let Some(actual) = log.last() else {
                return Err(NetworkError::Protocol(format!(
                    "device-writer state for {device_id} is empty"
                )));
            };
            if actual.body.device_sequence != expected.device_sequence
                || actual.entry_hash() != expected.entry_hash
            {
                return Err(NetworkError::Protocol(format!(
                    "device-writer delta did not reach advertised head for {device_id}"
                )));
            }
        }
    }

    let merged = merge_device_writer_logs(&authority, accumulated.values().cloned())
        .map_err(|error| NetworkError::Protocol(error.to_string()))?;
    Ok(super::CachedDeviceWriterState {
        authority_sequence: authority.latest_sequence,
        merged,
        last_verified_at,
        source_peer,
        authority_records,
        device_logs: accumulated,
    })
}

impl NetworkNode {
    pub(super) fn schedule_device_writer_sync_verification(
        &mut self,
        pending: PendingDeviceWriterSync,
        response: DeviceWriterSyncResponse,
    ) {
        let mut encoded_response = Vec::new();
        ciborium::into_writer(&response, &mut encoded_response)
            .expect("device-writer sync responses serialize to CBOR");
        self.device_writer_sync_work.received_entries +=
            response.device_logs.iter().map(Vec::len).sum::<usize>() as u64;
        self.device_writer_sync_work.received_bytes += encoded_response.len() as u64;
        if response.sync_version >= DELTA_DEVICE_WRITER_SYNC_VERSION {
            self.device_writer_sync_work.delta_responses += 1;
            if response.continuation.is_some() {
                self.device_writer_sync_work.delta_continuations += 1;
            }
        } else {
            self.device_writer_sync_work.full_responses += 1;
            if pending.requested_sync_version >= DELTA_DEVICE_WRITER_SYNC_VERSION
                && pending.requested_cursor_count > 0
            {
                self.device_writer_sync_work.full_recoveries += 1;
            }
        }
        let id = self.device_writer_sync_work.next_id;
        self.device_writer_sync_work.next_id = self.device_writer_sync_work.next_id.wrapping_add(1);
        let work = DeviceWriterSyncWork {
            id,
            existing: self.device_writer_states.get(&pending.identity).cloned(),
            identity: pending.identity.clone(),
            provider: pending.provider,
            deadline: pending.deadline,
            settled: false,
            authority_records: response.authority_records,
            device_logs: response.device_logs,
            sync_version: response.sync_version,
            heads: response.heads,
            continuation: response.continuation,
        };

        if self.device_writer_sync_work.active.len() < self.device_writer_sync_work.max_concurrency
        {
            self.device_writer_sync_work
                .by_identity
                .insert(pending.identity, id);
            self.spawn_device_writer_sync_verification(work);
        } else if self.device_writer_sync_work.queued.len()
            < self.device_writer_sync_work.queue_capacity
        {
            self.device_writer_sync_work
                .by_identity
                .insert(pending.identity, id);
            self.device_writer_sync_work.queued.push_back(work);
        } else {
            self.device_writer_sync_work.rejected += 1;
            self.answer_device_writer_waiters_with_outcome(
                &pending.identity,
                MaterializedRecordRefreshOutcome::Overloaded,
            );
            self.retry_pending_local_device_writer_refresh();
        }
    }

    fn spawn_device_writer_sync_verification(&mut self, work: DeviceWriterSyncWork) {
        let id = work.id;
        let identity = work.identity.clone();
        let base_state = work.existing.clone();
        let continuation = work.continuation.clone();
        self.device_writer_sync_work.active.insert(
            id,
            ActiveDeviceWriterSyncWork {
                identity: identity.clone(),
                provider: work.provider,
                deadline: work.deadline,
                settled: work.settled,
                base_state,
                continuation,
            },
        );
        let completion_tx = self.device_writer_sync_work.completion_tx.clone();
        tokio::task::spawn_blocking(move || {
            let result = merge_verified_device_writer_state(
                &identity,
                work.existing,
                work.authority_records,
                work.device_logs,
                work.sync_version,
                work.heads,
                work.continuation.is_none(),
            )
            .map(|mut state| {
                state.last_verified_at = super::unix_now();
                state.source_peer = Some(work.provider.to_string());
                state
            });
            let _ = completion_tx.blocking_send(CompletedDeviceWriterSyncWork { id, result });
        });
    }

    pub(super) fn handle_device_writer_sync_completion(
        &mut self,
        completion: CompletedDeviceWriterSyncWork,
    ) {
        let Some(mut active) = self.device_writer_sync_work.active.remove(&completion.id) else {
            return;
        };
        let owns_identity = self
            .device_writer_sync_work
            .by_identity
            .get(&active.identity)
            .is_some_and(|id| *id == completion.id);
        if owns_identity {
            self.device_writer_sync_work
                .by_identity
                .remove(&active.identity);
        }

        if !active.settled && active.deadline <= Instant::now() {
            active.settled = true;
            self.device_writer_sync_work.timed_out += 1;
            if owns_identity {
                self.answer_device_writer_waiters_with_outcome(
                    &active.identity,
                    MaterializedRecordRefreshOutcome::NetworkUnavailable,
                );
            }
        }

        match completion.result {
            Ok(state) => {
                let snapshot_is_current = match (
                    active.base_state.as_ref(),
                    self.device_writer_states.get(&active.identity),
                ) {
                    (None, None) => true,
                    (Some(base), Some(current)) => Arc::ptr_eq(base, current),
                    _ => false,
                };
                if snapshot_is_current {
                    let state = Arc::new(state);
                    if active.identity != self.identity.identity_id() {
                        if let Err(error) = self
                            .remote_identity_persistence
                            .enqueue(active.identity.clone(), state.clone())
                        {
                            tracing::warn!(
                                identity = %active.identity,
                                %error,
                                "verified remote identity state could not be queued for persistence"
                            );
                        }
                    }
                    self.device_writer_states
                        .insert(active.identity.clone(), state);
                }
                self.device_writer_sync_work.verified += 1;

                let continuation = active.continuation.take();
                let continue_sync = snapshot_is_current
                    && owns_identity
                    && !active.settled
                    && active.deadline > Instant::now()
                    && continuation.is_some();
                if continue_sync {
                    self.request_device_writer_sync_continuation(
                        active.identity.clone(),
                        &active.provider,
                        continuation.expect("continuation was checked"),
                        active.deadline,
                    );
                }
                let has_waiters = self
                    .pending_device_writer_waiters
                    .contains_key(&active.identity);
                if !continue_sync && owns_identity && (!active.settled || has_waiters) {
                    self.answer_device_writer_waiters(&active.identity);
                }
            }
            Err(error) => {
                self.device_writer_sync_work.verification_failed += 1;
                let has_waiters = self
                    .pending_device_writer_waiters
                    .contains_key(&active.identity);
                if owns_identity && (!active.settled || has_waiters) {
                    let error =
                        Self::identity_head_invalid_failure(&active.identity, error.to_string());
                    self.on_device_writer_sync_settled(
                        &active.identity,
                        &active.provider,
                        Some(error),
                    );
                }
            }
        }
        self.retry_pending_local_device_writer_refresh();
        self.start_queued_device_writer_sync_work();
    }

    fn start_queued_device_writer_sync_work(&mut self) {
        while self.device_writer_sync_work.active.len()
            < self.device_writer_sync_work.max_concurrency
        {
            let Some(work) = self.device_writer_sync_work.queued.pop_front() else {
                break;
            };
            self.spawn_device_writer_sync_verification(work);
        }
    }

    #[cfg(test)]
    async fn complete_next_device_writer_sync_work(&mut self) {
        let completion = self
            .device_writer_sync_work
            .completion_rx
            .recv()
            .await
            .expect("scheduled device-writer verification must complete");
        self.handle_device_writer_sync_completion(completion);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use jolt_core::{
        ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
        DeviceWriterOperation, DeviceWriterPathMode, JoltAddress, UpdateAction, UpdateLogEntry,
    };
    use jolt_identity::NodeIdentity;
    use jolt_store::{CacheConfig, ContentStore};
    use libp2p::request_response;
    use libp2p::swarm::SwarmEvent;
    use tempfile::tempdir;
    use tokio::sync::oneshot;

    use crate::behaviour::JoltBehaviourEvent;
    use crate::command::DaemonCommand;
    use crate::config::NetworkConfig;
    use crate::error::NetworkError;
    use crate::node::NetworkNode;
    use crate::protocol::{
        DeviceWriterCursor, DeviceWriterSyncContinuation, DeviceWriterSyncResponse,
        UpdateLogResponse, DELTA_DEVICE_WRITER_SYNC_VERSION,
    };

    /// Build a single-device writer log of append records for `identity`, signed
    /// by `device`, one entry per `(path, content_id)` pair.
    fn append_log(
        identity: &jolt_core::IdentityId,
        device: &NodeIdentity,
        device_id: &str,
        records: &[(&str, ContentId)],
    ) -> Vec<DeviceWriterLogEntry> {
        let mut entries: Vec<DeviceWriterLogEntry> = Vec::new();
        for (index, (path, content_id)) in records.iter().enumerate() {
            let operation = DeviceWriterOperation::append_record(*path, content_id.clone());
            let created_at = 100 + index as u64;
            let entry = match entries.last() {
                None => DeviceWriterLogEntry::genesis(
                    identity.clone(),
                    device_id,
                    operation,
                    created_at,
                    |bytes| device.sign(bytes),
                )
                .unwrap(),
                Some(previous) => previous
                    .append(operation, created_at, |bytes| device.sign(bytes))
                    .unwrap(),
            };
            entries.push(entry);
        }
        entries
    }

    fn large_append_log(
        identity: &jolt_core::IdentityId,
        device: &NodeIdentity,
        device_id: &str,
        count: usize,
    ) -> Vec<DeviceWriterLogEntry> {
        let mut entries: Vec<DeviceWriterLogEntry> = Vec::with_capacity(count);
        for index in 0..count {
            let operation = DeviceWriterOperation::append_record(
                format!("/load/items/{index:05}"),
                ContentId::from_bytes(format!("item-{index}").as_bytes()),
            );
            let entry = match entries.last() {
                None => DeviceWriterLogEntry::genesis(
                    identity.clone(),
                    device_id,
                    operation,
                    100,
                    |bytes| device.sign(bytes),
                )
                .unwrap(),
                Some(previous) => previous
                    .append(operation, 100 + index as u64, |bytes| device.sign(bytes))
                    .unwrap(),
            };
            entries.push(entry);
        }
        entries
    }

    #[test]
    fn only_an_active_authorized_device_peer_is_a_same_owner_sync_peer() {
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let stranger = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = format!("dev_{}", device.identity_id());
        let authorized = DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                device_id.clone(),
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Sibling".to_string()),
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap();

        assert!(super::authority_records_authorize_peer(
            &identity,
            std::slice::from_ref(&authorized),
            &device.peer_id(),
        ));
        assert!(!super::authority_records_authorize_peer(
            &identity,
            std::slice::from_ref(&authorized),
            &stranger.peer_id(),
        ));

        let revoked = authorized
            .append(
                DeviceAuthorizationOperation::revoke_device(
                    device_id,
                    None,
                    Some("Retired".to_string()),
                    101,
                ),
                101,
                |bytes| root.sign(bytes),
            )
            .unwrap();
        assert!(!super::authority_records_authorize_peer(
            &identity,
            &[authorized, revoked],
            &device.peer_id(),
        ));
    }

    /// Drive a device-writer sync triggered by a daemon command: feed the
    /// in-flight `device_writer_sync` request a provider response carrying the
    /// supplied authority records and device logs.
    fn receive_device_writer_sync_response(
        node: &mut NetworkNode,
        provider: libp2p::PeerId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) {
        receive_device_writer_sync_page(
            node,
            provider,
            DeviceWriterSyncResponse {
                required_operation_version: crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                sync_version: crate::protocol::LEGACY_DEVICE_WRITER_SYNC_VERSION,
                heads: Vec::new(),
                continuation: None,
                authority_records,
                device_logs,
            },
        );
    }

    fn receive_device_writer_sync_page(
        node: &mut NetworkNode,
        provider: libp2p::PeerId,
        response: DeviceWriterSyncResponse,
    ) {
        let request_id = *node
            .pending_device_writer_syncs
            .keys()
            .next()
            .expect("a device-writer sync request should be in flight");
        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::DeviceWriterSync(
            request_response::Event::Message {
                peer: provider,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                message: request_response::Message::Response {
                    request_id,
                    response,
                },
            },
        )));
    }

    async fn deliver_device_writer_sync_response(
        node: &mut NetworkNode,
        provider: libp2p::PeerId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) {
        receive_device_writer_sync_response(node, provider, authority_records, device_logs);
        if !node.device_writer_sync_work.active.is_empty() {
            node.complete_next_device_writer_sync_work().await;
        }
    }

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
    }

    fn make_node_with_config(dir: &std::path::Path, config: NetworkConfig) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, config).unwrap()
    }

    fn authorize_device(
        root: &NodeIdentity,
        device: &NodeIdentity,
        device_id: &str,
    ) -> DeviceAuthorizationRecord {
        DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            root.identity_id(),
            DeviceAuthorizationOperation::authorize_device(
                device_id,
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Laptop".to_string()),
                1_780_579_200,
            ),
            1_780_579_200,
            |bytes| root.sign(bytes),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn daemon_resolution_retries_next_update_log_provider_after_dial_failure() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let address = JoltAddress::new(owner.identity_id(), "/profile").unwrap();
        let failed_provider = libp2p::PeerId::random();
        let fallback_provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(address.identity());
        node.discovered_providers
            .insert(key, vec![failed_provider, fallback_provider]);

        let (tx, mut rx) = oneshot::channel();
        node.request_daemon_resolve_from_provider(
            address.clone(),
            None,
            &failed_provider,
            tx,
            None,
        );
        let failed_request_id = *node.pending_daemon_resolutions.keys().next().unwrap();

        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
            request_response::Event::OutboundFailure {
                peer: failed_provider,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                request_id: failed_request_id,
                error: request_response::OutboundFailure::DialFailure,
            },
        )));

        assert!(matches!(
            rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        let pending = node.pending_daemon_resolutions.values().next().unwrap();
        assert_eq!(pending.provider, fallback_provider);
    }

    #[tokio::test]
    async fn daemon_resolution_returns_cached_address_before_provider_refresh() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let identity = owner.identity_id();
        let path = "/hello/1234";
        let old_content_id = ContentId::from_bytes(b"old content");
        let new_content_id = ContentId::from_bytes(b"new content");
        let genesis = UpdateLogEntry::genesis(
            owner.public_key_bytes(),
            UpdateAction::SetPath {
                path: path.to_string(),
                content_id: old_content_id.clone(),
            },
            |bytes| owner.sign(bytes),
        )
        .unwrap();
        let newer = genesis
            .append(
                UpdateAction::SetPath {
                    path: path.to_string(),
                    content_id: new_content_id.clone(),
                },
                |bytes| owner.sign(bytes),
            )
            .unwrap();
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        let address = JoltAddress::new(identity.clone(), path).unwrap();

        node.store_verified_update_log(identity.clone(), vec![genesis.clone()])
            .unwrap();
        node.discovered_providers.insert(key, vec![provider]);

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });

        let resolved = rx.await.unwrap().unwrap();
        assert_eq!(resolved.content_id, old_content_id.to_string());
        assert_eq!(resolved.latest_sequence, 0);
        assert_eq!(resolved.source, "cache");
        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        let (request_id, pending) = node.pending_daemon_resolutions.iter().next().unwrap();
        assert!(pending.response_tx.is_none());
        let request_id = *request_id;

        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::UpdateLogSync(
            request_response::Event::Message {
                peer: provider,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                message: request_response::Message::Response {
                    request_id,
                    response: UpdateLogResponse {
                        entries: vec![genesis, newer],
                    },
                },
            },
        )));

        let refreshed = node
            .resolve_response_from_cache(&address, None, "cache")
            .unwrap();
        assert_eq!(refreshed.content_id, new_content_id.to_string());
        assert_eq!(refreshed.latest_sequence, 1);
    }

    #[tokio::test]
    async fn repeated_cached_resolves_coalesce_background_identity_refreshes() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let identity = owner.identity_id();
        let content_id = ContentId::from_bytes(b"cached content");
        let entry = UpdateLogEntry::genesis(
            owner.public_key_bytes(),
            UpdateAction::SetPath {
                path: "/posts/1".to_string(),
                content_id: content_id.clone(),
            },
            |bytes| owner.sign(bytes),
        )
        .unwrap();
        let address = JoltAddress::new(identity.clone(), "/posts/1").unwrap();
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);

        node.store_verified_update_log(identity.clone(), vec![entry])
            .unwrap();

        for _ in 0..3 {
            // Relay discovery can report the same provider again after the
            // resolve path consumes it from the shared candidate pool.
            let providers = node.discovered_providers.entry(key.clone()).or_default();
            if !providers.contains(&provider) {
                providers.push(provider);
            }

            let (tx, rx) = oneshot::channel();
            node.handle_command(DaemonCommand::Resolve {
                address: address.to_string(),
                response_tx: tx,
            });
            let resolved = rx.await.unwrap().unwrap();
            assert_eq!(resolved.content_id, content_id.to_string());
            assert_eq!(resolved.source, "cache");
        }

        assert_eq!(
            node.pending_daemon_resolutions.len(),
            1,
            "one identity should have at most one background update-log refresh"
        );
        assert_eq!(node.pending_device_writer_syncs.len(), 1);
        assert_eq!(
            node.pending_device_writer_waiters
                .get(&identity)
                .map(Vec::len),
            Some(1),
            "duplicate cache hits must not accumulate no-op refresh waiters"
        );
    }

    #[tokio::test]
    async fn cached_resolve_does_not_immediately_retry_empty_device_writer_sync() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let owner = NodeIdentity::generate();
        let identity = owner.identity_id();
        let content_id = ContentId::from_bytes(b"legacy cached content");
        let entry = UpdateLogEntry::genesis(
            owner.public_key_bytes(),
            UpdateAction::SetPath {
                path: "/posts/1".to_string(),
                content_id,
            },
            |bytes| owner.sign(bytes),
        )
        .unwrap();
        let address = JoltAddress::new(identity.clone(), "/posts/1").unwrap();
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);

        node.store_verified_update_log(identity.clone(), vec![entry])
            .unwrap();
        node.discovered_providers
            .insert(key.clone(), vec![provider]);

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });
        rx.await.unwrap().unwrap();

        deliver_device_writer_sync_response(&mut node, provider, Vec::new(), Vec::new()).await;
        assert!(node.pending_device_writer_syncs.is_empty());

        // A relay may advertise the legacy update-log provider again. The
        // empty device-writer response is still fresh knowledge and should not
        // trigger another sync for every path opened under this identity.
        node.discovered_providers
            .entry(key)
            .or_default()
            .push(provider);
        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });
        rx.await.unwrap().unwrap();

        assert!(
            node.pending_device_writer_syncs.is_empty(),
            "an empty device-writer response should suppress immediate retries"
        );
    }

    #[test]
    fn cached_identity_refresh_cooldown_expires() {
        let identity = NodeIdentity::generate().identity_id();
        let mut refreshes = HashMap::new();

        assert!(NetworkNode::mark_refresh_if_due(&mut refreshes, &identity));
        assert!(!NetworkNode::mark_refresh_if_due(&mut refreshes, &identity));

        refreshes.insert(
            identity.clone(),
            Instant::now() - super::super::CACHED_IDENTITY_REFRESH_INTERVAL,
        );
        assert!(NetworkNode::mark_refresh_if_due(&mut refreshes, &identity));
    }

    #[tokio::test]
    async fn daemon_resolution_uses_cached_device_writer_state() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let content_id = ContentId::from_bytes(b"profile from laptop device log");
        let device_log = vec![DeviceWriterLogEntry::genesis(
            identity.clone(),
            device_id,
            DeviceWriterOperation::set_path(
                "/profile",
                content_id.clone(),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| laptop.sign(bytes),
        )
        .unwrap()];
        let address = JoltAddress::new(identity.clone(), "/profile").unwrap();

        node.store_verified_device_writer_logs(identity, authority, vec![device_log])
            .unwrap();

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });

        let resolved = rx.await.unwrap().unwrap();
        assert_eq!(resolved.content_id, content_id.to_string());
        assert_eq!(resolved.path, "/profile");
        assert_eq!(resolved.source, "device_writer_cache");
    }

    #[tokio::test]
    async fn daemon_resolution_does_not_resurrect_tombstoned_legacy_content() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let path = "/posts/post-1";
        let old_content_id = ContentId::from_bytes(b"deleted post");
        let legacy_entry = UpdateLogEntry::genesis(
            root.public_key_bytes(),
            UpdateAction::SetPath {
                path: path.to_string(),
                content_id: old_content_id.clone(),
            },
            |bytes| root.sign(bytes),
        )
        .unwrap();
        node.store_verified_update_log(identity.clone(), vec![legacy_entry])
            .unwrap();

        let authority = vec![authorize_device(&root, &laptop, "dev_laptop")];
        let present = DeviceWriterLogEntry::genesis(
            identity.clone(),
            "dev_laptop",
            DeviceWriterOperation::set_path(path, old_content_id, DeviceWriterPathMode::Singleton),
            100,
            |bytes| laptop.sign(bytes),
        )
        .unwrap();
        let tombstone = present
            .append(DeviceWriterOperation::tombstone_path(path), 101, |bytes| {
                laptop.sign(bytes)
            })
            .unwrap();
        node.store_verified_device_writer_logs(
            identity.clone(),
            authority,
            vec![vec![present, tombstone]],
        )
        .unwrap();

        let address = JoltAddress::new(identity, path).unwrap();
        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: tx,
        });

        let error = rx.await.unwrap().unwrap_err();
        assert!(matches!(
            error,
            NetworkError::PathTombstoned { path: deleted_path } if deleted_path == path
        ));
        assert!(node.pending_daemon_resolutions.is_empty());
    }

    #[tokio::test]
    async fn daemon_store_device_writer_logs_command_updates_resolve_cache() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let content_id = ContentId::from_bytes(b"profile stored through daemon command");
        let device_log = vec![DeviceWriterLogEntry::genesis(
            identity.clone(),
            device_id,
            DeviceWriterOperation::set_path(
                "/profile",
                content_id.clone(),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| laptop.sign(bytes),
        )
        .unwrap()];
        let address = JoltAddress::new(identity.clone(), "/profile").unwrap();

        let (store_tx, store_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::StoreDeviceWriterLogs {
            identity,
            authority_records: authority,
            device_logs: vec![device_log],
            response_tx: store_tx,
        });

        assert_eq!(store_rx.await.unwrap().unwrap(), 0);

        let (resolve_tx, resolve_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address: address.to_string(),
            response_tx: resolve_tx,
        });

        let resolved = resolve_rx.await.unwrap().unwrap();
        assert_eq!(resolved.content_id, content_id.to_string());
        assert_eq!(resolved.source, "device_writer_cache");
    }

    #[tokio::test]
    async fn daemon_publish_path_populates_device_writer_resolve_cache() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let file_path = dir.path().join("profile.txt");
        std::fs::write(
            &file_path,
            b"profile published through normal daemon command",
        )
        .unwrap();

        let (publish_tx, publish_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Publish {
            file_path,
            path: Some("/profile".to_string()),
            response_tx: publish_tx,
        });
        let published = publish_rx.await.unwrap().unwrap();
        let address = published.address.unwrap();

        let (resolve_tx, resolve_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::Resolve {
            address,
            response_tx: resolve_tx,
        });

        let resolved = resolve_rx.await.unwrap().unwrap();
        assert_eq!(resolved.content_id, published.content_id);
        assert_eq!(resolved.path, "/profile");
        assert_eq!(resolved.source, "device_writer_cache");
    }

    #[tokio::test]
    async fn daemon_append_publish_records_coexist_under_prefix() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let identity = node.identity.identity_id();

        for (name, path, body) in [
            ("a.txt", "/app/items/1", b"record one".as_slice()),
            ("b.txt", "/app/items/2", b"record two".as_slice()),
        ] {
            let file_path = dir.path().join(name);
            std::fs::write(&file_path, body).unwrap();
            let (tx, rx) = oneshot::channel();
            node.handle_command(DaemonCommand::PublishAppend {
                file_path,
                path: path.to_string(),
                response_tx: tx,
            });
            let response = rx.await.unwrap().unwrap();
            assert_eq!(response.path.as_deref(), Some(path));
        }

        // Append publish must never write the last-writer-wins update log.
        assert!(node.update_log_entries(&identity).is_none());

        let state = node.device_writer_states.get(&identity).unwrap();
        let records = state.merged.append_records_under("/app/items/");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].0, "/app/items/1");
        assert_eq!(records[1].0, "/app/items/2");
    }

    #[tokio::test]
    async fn daemon_enumerate_append_records_filters_remote_identity_by_prefix() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let record = ContentId::from_bytes(b"remote record");
        let unrelated = ContentId::from_bytes(b"unrelated record");
        let genesis = DeviceWriterLogEntry::genesis(
            identity.clone(),
            device_id,
            DeviceWriterOperation::append_record("/app/items/1", record.clone()),
            100,
            |bytes| laptop.sign(bytes),
        )
        .unwrap();
        let second = genesis
            .append(
                DeviceWriterOperation::append_record("/app/other/x", unrelated),
                101,
                |bytes| laptop.sign(bytes),
            )
            .unwrap();
        node.store_verified_device_writer_logs(
            identity.clone(),
            authority,
            vec![vec![genesis, second]],
        )
        .unwrap();

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/app/items/".to_string(),
            response_tx: tx,
        });
        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "/app/items/1");
        assert_eq!(records[0].content_id, record.to_string());
        assert_eq!(records[0].device_id, device_id);

        // Unknown identity yields an empty list rather than an error.
        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: NodeIdentity::generate().identity_id(),
            path_prefix: "/".to_string(),
            response_tx: tx,
        });
        assert!(rx.await.unwrap().unwrap().is_empty());
    }

    #[tokio::test]
    async fn materialized_record_view_reports_unavailable_without_verified_state() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let identity = NodeIdentity::generate().identity_id();

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::RefreshMaterializedRecordView {
            identity,
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });

        let view = rx.await.unwrap().unwrap();
        assert!(view.records.is_empty());
        assert_eq!(view.last_verified_at, None);
        assert_eq!(
            view.refresh,
            crate::command::MaterializedRecordRefreshOutcome::NetworkUnavailable
        );
    }

    #[tokio::test]
    async fn remote_append_records_become_enumerable_after_device_writer_sync() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let post_a = ContentId::from_bytes(b"spoke post a");
        let post_b = ContentId::from_bytes(b"spoke post b");
        let device_log = append_log(
            &identity,
            &laptop,
            device_id,
            &[
                ("/spoke/posts/1", post_a.clone()),
                ("/spoke/posts/2", post_b.clone()),
            ],
        );

        // A discovered provider is known for the remote identity, but no
        // device-writer state is cached yet: a fresh reader sees nothing.
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        node.discovered_providers.insert(key, vec![provider]);

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });
        // The enumerate is parked behind a device-writer sync, not answered yet.
        assert_eq!(node.pending_device_writer_syncs.len(), 1);

        deliver_device_writer_sync_response(&mut node, provider, authority, vec![device_log]).await;

        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].path, "/spoke/posts/1");
        assert_eq!(records[0].content_id, post_a.to_string());
        assert_eq!(records[1].path, "/spoke/posts/2");
        assert_eq!(records[1].content_id, post_b.to_string());
        assert_eq!(records[0].device_id, device_id);
        node.remote_identity_persistence.flush();
        let persisted = node.store.load_remote_identity_states().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].identity, identity);
        assert_eq!(persisted[0].source_peer, Some(provider.to_string()));
        assert!(persisted[0].last_verified_at > 0);
    }

    #[tokio::test]
    async fn remote_logical_records_become_materialized_after_device_writer_sync() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let first_content = ContentId::from_bytes(b"first stable post");
        let second_content = ContentId::from_bytes(b"second stable post");
        let first = DeviceWriterLogEntry::genesis(
            identity.clone(),
            device_id,
            DeviceWriterOperation::set_path(
                "/spoke/posts/1",
                first_content.clone(),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| laptop.sign(bytes),
        )
        .unwrap();
        let second = first
            .append(
                DeviceWriterOperation::set_path(
                    "/spoke/posts/2",
                    second_content.clone(),
                    DeviceWriterPathMode::Singleton,
                ),
                101,
                |bytes| laptop.sign(bytes),
            )
            .unwrap();
        let provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&identity),
            vec![provider],
        );

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::RefreshMaterializedRecordView {
            identity: identity.clone(),
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });
        deliver_device_writer_sync_response(
            &mut node,
            provider,
            authority,
            vec![vec![first, second]],
        )
        .await;

        let view = rx.await.unwrap().unwrap();
        assert_eq!(view.records.len(), 2);
        assert_eq!(view.records[0].path, "/spoke/posts/1");
        assert_eq!(view.records[0].content_id, first_content.to_string());
        assert_eq!(view.records[1].path, "/spoke/posts/2");
        assert_eq!(view.records[1].content_id, second_content.to_string());
        assert!(view.last_verified_at.is_some());
        assert_eq!(
            view.refresh,
            crate::command::MaterializedRecordRefreshOutcome::Ready
        );
    }

    #[test]
    fn persistence_worker_coalesces_a_burst_to_the_latest_identity_snapshot() {
        let dir = tempdir().unwrap();
        let store = ContentStore::open(dir.path(), CacheConfig::default()).unwrap();
        let mut worker =
            super::RemoteIdentityPersistenceWorker::new(store.remote_identity_state_store());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![authorize_device(&root, &laptop, "dev_burst")];

        for last_verified_at in 1..=128 {
            let mut state = super::merge_verified_device_writer_state(
                &identity,
                None,
                authority.clone(),
                Vec::new(),
                crate::protocol::LEGACY_DEVICE_WRITER_SYNC_VERSION,
                Vec::new(),
                true,
            )
            .unwrap();
            state.last_verified_at = last_verified_at;
            worker.enqueue(identity.clone(), Arc::new(state)).unwrap();
        }

        worker.flush();
        let persisted = store.load_remote_identity_states().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].identity, identity);
        assert_eq!(persisted[0].last_verified_at, 128);
        worker.shutdown();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn large_remote_sync_does_not_starve_unrelated_daemon_command() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_large_remote";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let cached_log = large_append_log(&identity, &laptop, device_id, 100);
        node.store_verified_device_writer_logs(
            identity.clone(),
            authority.clone(),
            vec![cached_log],
        )
        .unwrap();
        let device_log = large_append_log(&identity, &laptop, device_id, 101);
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        node.discovered_providers.insert(key, vec![provider]);

        node.begin_device_writer_sync(super::DeviceWriterSyncWaiter::Refresh {
            identity: identity.clone(),
        });

        let started = Instant::now();
        receive_device_writer_sync_response(&mut node, provider, authority, vec![device_log]);
        assert_eq!(node.device_writer_sync_work.active.len(), 1);
        let (status_tx, status_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::GetStatus {
            response_tx: status_tx,
        });
        status_rx.await.unwrap();
        let scheduling_delay = started.elapsed();
        assert!(
            scheduling_delay < Duration::from_millis(250),
            "large remote verification blocked an unrelated daemon command for {scheduling_delay:?}"
        );

        let (coalesced_tx, coalesced_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/load/items/".to_string(),
            response_tx: coalesced_tx,
        });
        assert!(node.pending_device_writer_syncs.is_empty());
        assert_eq!(node.device_writer_sync_work.by_identity.len(), 1);

        node.complete_next_device_writer_sync_work().await;
        assert_eq!(coalesced_rx.await.unwrap().unwrap().len(), 101);
        assert_eq!(
            node.enumerate_append_records(&identity, "/load/items/")
                .unwrap()
                .len(),
            101,
            "bounded scheduling must preserve the verified merge result"
        );
    }

    #[tokio::test]
    async fn sync_work_limits_reject_overload_and_report_status() {
        let dir = tempdir().unwrap();
        let mut config = NetworkConfig::test_config();
        config.device_writer_sync_max_concurrency = 1;
        config.device_writer_sync_queue_capacity = 1;
        let mut node = make_node_with_config(dir.path(), config);
        let mut overloaded_rx = None;

        for index in 0..3 {
            let root = NodeIdentity::generate();
            let device = NodeIdentity::generate();
            let identity = root.identity_id();
            let device_id = format!("dev_{index}");
            let authority = vec![authorize_device(&root, &device, &device_id)];
            let device_log = large_append_log(&identity, &device, &device_id, 1);
            let provider = libp2p::PeerId::random();
            node.discovered_providers.insert(
                NetworkNode::update_log_provider_key(&identity),
                vec![provider],
            );
            if index == 2 {
                let (response_tx, response_rx) = oneshot::channel();
                overloaded_rx = Some(response_rx);
                node.handle_command(DaemonCommand::EnumerateAppendRecords {
                    identity: identity.clone(),
                    path_prefix: "/load/items/".to_string(),
                    response_tx,
                });
            } else {
                node.begin_device_writer_sync(super::DeviceWriterSyncWaiter::Refresh {
                    identity: identity.clone(),
                });
            }
            receive_device_writer_sync_response(&mut node, provider, authority, vec![device_log]);
        }

        assert!(overloaded_rx.unwrap().await.unwrap().unwrap().is_empty());
        let status = node.build_status().device_writer_sync_work;
        assert_eq!(status.max_concurrency, 1);
        assert_eq!(status.queue_capacity, 1);
        assert_eq!(status.active, 1);
        assert_eq!(status.queued, 1);
        assert_eq!(status.rejected, 1);

        node.complete_next_device_writer_sync_work().await;
        node.complete_next_device_writer_sync_work().await;
        let status = node.build_status().device_writer_sync_work;
        assert_eq!(status.active, 0);
        assert_eq!(status.queued, 0);
        assert_eq!(status.verified, 2);
        assert_eq!(status.verification_failed, 0);
    }

    #[tokio::test]
    async fn invalid_remote_sync_keeps_the_last_verified_view_unchanged() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let authorized_device = NodeIdentity::generate();
        let attacker = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_authorized";
        let authority = vec![authorize_device(&root, &authorized_device, device_id)];
        let invalid_log = vec![DeviceWriterLogEntry::genesis(
            identity.clone(),
            device_id,
            DeviceWriterOperation::append_record(
                "/load/items/invalid",
                ContentId::from_bytes(b"invalid"),
            ),
            100,
            |bytes| attacker.sign(bytes),
        )
        .unwrap()];
        let provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&identity),
            vec![provider],
        );
        let (response_tx, response_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/load/items/".to_string(),
            response_tx,
        });
        receive_device_writer_sync_response(&mut node, provider, authority, vec![invalid_log]);
        node.complete_next_device_writer_sync_work().await;

        assert!(response_rx.await.unwrap().unwrap().is_empty());
        assert!(!node.has_device_writer_state(&identity));
        let status = node.build_status().device_writer_sync_work;
        assert_eq!(status.verified, 0);
        assert_eq!(status.verification_failed, 1);
    }

    #[tokio::test]
    async fn sync_timeout_cancellation_and_shutdown_release_waiters() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());

        let cancelled_identity = NodeIdentity::generate().identity_id();
        let cancelled_provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&cancelled_identity),
            vec![cancelled_provider],
        );
        let (cancelled_tx, cancelled_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: cancelled_identity.clone(),
            path_prefix: "/".to_string(),
            response_tx: cancelled_tx,
        });
        drop(cancelled_rx);
        node.check_device_writer_sync_timeouts();
        assert!(!node
            .pending_device_writer_waiters
            .contains_key(&cancelled_identity));
        assert!(!node
            .pending_device_writer_syncs
            .values()
            .any(|pending| pending.identity == cancelled_identity));
        assert_eq!(node.build_status().device_writer_sync_work.cancelled, 1);

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let timed_out_identity = root.identity_id();
        let timed_out_provider = libp2p::PeerId::random();
        let authority = vec![authorize_device(&root, &device, "dev_timeout")];
        let device_log = large_append_log(&timed_out_identity, &device, "dev_timeout", 100);
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&timed_out_identity),
            vec![timed_out_provider],
        );
        node.set_resolve_timeout(Duration::ZERO);
        let (timed_out_tx, timed_out_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: timed_out_identity.clone(),
            path_prefix: "/load/items/".to_string(),
            response_tx: timed_out_tx,
        });
        receive_device_writer_sync_response(
            &mut node,
            timed_out_provider,
            authority,
            vec![device_log],
        );
        node.check_device_writer_sync_timeouts();
        assert!(timed_out_rx.await.unwrap().unwrap().is_empty());
        assert_eq!(node.build_status().device_writer_sync_work.timed_out, 1);

        let shutdown_identity = NodeIdentity::generate().identity_id();
        let shutdown_provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&shutdown_identity),
            vec![shutdown_provider],
        );
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: shutdown_identity,
            path_prefix: "/".to_string(),
            response_tx: shutdown_tx,
        });
        node.shutdown_device_writer_sync_work();
        assert!(matches!(
            shutdown_rx.await.unwrap(),
            Err(NetworkError::ShuttingDown)
        ));

        node.complete_next_device_writer_sync_work().await;
        assert!(
            node.has_device_writer_state(&timed_out_identity),
            "valid work that misses the waiter deadline must still advance verified state"
        );
        assert_eq!(
            node.enumerate_append_records(&timed_out_identity, "/load/items/")
                .unwrap()
                .len(),
            100
        );
    }

    #[tokio::test]
    async fn stale_sync_snapshot_cannot_overwrite_newer_verified_state() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_snapshot_race";
        let authority = vec![authorize_device(&root, &device, device_id)];
        let stale_log = large_append_log(&identity, &device, device_id, 1);
        let newer_log = large_append_log(&identity, &device, device_id, 2);
        let provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&identity),
            vec![provider],
        );

        node.begin_device_writer_sync(super::DeviceWriterSyncWaiter::Refresh {
            identity: identity.clone(),
        });
        receive_device_writer_sync_response(
            &mut node,
            provider,
            authority.clone(),
            vec![stale_log],
        );
        node.store_verified_device_writer_logs(identity.clone(), authority, vec![newer_log])
            .unwrap();
        node.complete_next_device_writer_sync_work().await;

        assert_eq!(
            node.enumerate_append_records(&identity, "/load/items/")
                .unwrap()
                .len(),
            2,
            "a merge built from an old Arc snapshot must not replace newer verified state"
        );
    }

    #[tokio::test]
    async fn timed_out_queued_sync_still_advances_verified_state() {
        let dir = tempdir().unwrap();
        let mut config = NetworkConfig::test_config();
        config.device_writer_sync_max_concurrency = 1;
        config.device_writer_sync_queue_capacity = 1;
        let mut node = make_node_with_config(dir.path(), config);
        node.set_resolve_timeout(Duration::ZERO);
        let mut identities = Vec::new();

        for index in 0..2 {
            let root = NodeIdentity::generate();
            let device = NodeIdentity::generate();
            let identity = root.identity_id();
            let device_id = format!("dev_queued_timeout_{index}");
            let authority = vec![authorize_device(&root, &device, &device_id)];
            let log = large_append_log(&identity, &device, &device_id, 1);
            let provider = libp2p::PeerId::random();
            node.discovered_providers.insert(
                NetworkNode::update_log_provider_key(&identity),
                vec![provider],
            );
            node.begin_device_writer_sync(super::DeviceWriterSyncWaiter::Refresh {
                identity: identity.clone(),
            });
            receive_device_writer_sync_response(&mut node, provider, authority, vec![log]);
            identities.push(identity);
        }

        assert_eq!(node.device_writer_sync_work.active.len(), 1);
        assert_eq!(node.device_writer_sync_work.queued.len(), 1);
        node.check_device_writer_sync_timeouts();
        assert_eq!(node.build_status().device_writer_sync_work.timed_out, 2);

        node.complete_next_device_writer_sync_work().await;
        node.complete_next_device_writer_sync_work().await;
        for identity in identities {
            assert!(node.has_device_writer_state(&identity));
        }
        assert_eq!(node.build_status().device_writer_sync_work.verified, 2);
    }

    #[tokio::test]
    async fn remote_enumerate_refreshes_already_cached_device_writer_state() {
        // A peer that has already synced a remote identity's device-writer state
        // must still surface records the author appends *afterwards*. Regression:
        // enumerate short-circuited whenever any state was cached, so an author's
        // later appends (e.g. Spoke accepted-reply refs published after a reader's
        // first feed load) stayed invisible to peers who had synced earlier.
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];

        let post = ContentId::from_bytes(b"spoke post");
        let accepted = ContentId::from_bytes(b"spoke accepted reply ref");

        // A provider stays reachable for the whole test.
        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        node.discovered_providers.insert(key, vec![provider]);

        // First read: the author has published only the post. The reader syncs
        // and caches that single-record device log.
        let first_log = append_log(
            &identity,
            &laptop,
            device_id,
            &[("/spoke/posts/1", post.clone())],
        );
        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });
        assert_eq!(node.pending_device_writer_syncs.len(), 1);
        deliver_device_writer_sync_response(
            &mut node,
            provider,
            authority.clone(),
            vec![first_log],
        )
        .await;
        assert_eq!(rx.await.unwrap().unwrap().len(), 1);
        assert!(node.has_device_writer_state(&identity));

        // The author now appends an accepted-reply ref under a different prefix.
        let second_log = append_log(
            &identity,
            &laptop,
            device_id,
            &[
                ("/spoke/posts/1", post.clone()),
                ("/spoke/accepted/post_1/reply_1", accepted.clone()),
            ],
        );

        // The peer re-enumerates the accepted-reply Collection. Even though it
        // already holds cached state, it must refresh from the reachable provider
        // and surface the newly appended record rather than answering from the
        // stale cache.
        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/spoke/accepted/post_1/".to_string(),
            response_tx: tx,
        });
        assert_eq!(
            node.pending_device_writer_syncs.len(),
            1,
            "a cached-but-stale remote enumerate must trigger a refresh sync"
        );
        let delta = vec![second_log.last().cloned().unwrap()];
        assert_eq!(
            delta.len(),
            1,
            "the warm response transfers only the append"
        );
        deliver_device_writer_sync_response(&mut node, provider, authority, vec![delta]).await;

        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "/spoke/accepted/post_1/reply_1");
        assert_eq!(records[0].content_id, accepted.to_string());
        let status = node.build_status().device_writer_sync_work;
        assert_eq!(status.full_responses, 2);
        assert_eq!(
            status.full_recoveries, 1,
            "a legacy full-state response to a warm cursor request is observable recovery"
        );
    }

    #[tokio::test]
    async fn bounded_delta_pages_verify_before_answering_waiters() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_paged_delta";
        let authority = vec![authorize_device(&root, &device, device_id)];
        let full_log = append_log(
            &identity,
            &device,
            device_id,
            &[
                ("/load/items/first", ContentId::from_bytes(b"first")),
                ("/load/items/second", ContentId::from_bytes(b"second")),
            ],
        );
        let first = full_log[0].clone();
        let second = full_log[1].clone();
        let provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&identity),
            vec![provider],
        );
        let (response_tx, mut response_rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/load/items/".to_string(),
            response_tx,
        });

        receive_device_writer_sync_page(
            &mut node,
            provider,
            DeviceWriterSyncResponse {
                required_operation_version: crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
                heads: vec![DeviceWriterCursor::from_entry(&second)],
                continuation: Some(DeviceWriterSyncContinuation {
                    cursors: vec![DeviceWriterCursor::from_entry(&first)],
                }),
                authority_records: authority.clone(),
                device_logs: vec![vec![first]],
            },
        );
        node.complete_next_device_writer_sync_work().await;
        assert_eq!(node.pending_device_writer_syncs.len(), 1);
        assert!(matches!(
            response_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        receive_device_writer_sync_page(
            &mut node,
            provider,
            DeviceWriterSyncResponse {
                required_operation_version: crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
                heads: vec![DeviceWriterCursor::from_entry(&second)],
                continuation: None,
                authority_records: authority,
                device_logs: vec![vec![second]],
            },
        );
        node.complete_next_device_writer_sync_work().await;

        assert_eq!(response_rx.await.unwrap().unwrap().len(), 2);
        let status = node.build_status().device_writer_sync_work;
        assert_eq!(status.delta_responses, 2);
        assert_eq!(status.delta_continuations, 1);
        assert_eq!(status.received_entries, 2);
        assert!(status.received_bytes > 0);
        assert_eq!(status.full_recoveries, 0);
    }

    #[tokio::test]
    async fn discontinuous_or_incomplete_delta_keeps_last_verified_view() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_invalid_delta";
        let authority = vec![authorize_device(&root, &device, device_id)];
        let log = large_append_log(&identity, &device, device_id, 3);
        node.store_verified_device_writer_logs(
            identity.clone(),
            authority.clone(),
            vec![vec![log[0].clone()]],
        )
        .unwrap();
        let provider = libp2p::PeerId::random();
        node.discovered_providers.insert(
            NetworkNode::update_log_provider_key(&identity),
            vec![provider],
        );

        for (delta, advertised_head) in [
            (log[2].clone(), log[2].clone()),
            (log[1].clone(), log[0].clone()),
        ] {
            node.request_device_writer_sync_from_provider(identity.clone(), &provider);
            receive_device_writer_sync_page(
                &mut node,
                provider,
                DeviceWriterSyncResponse {
                    required_operation_version:
                        crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                    sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
                    heads: vec![DeviceWriterCursor::from_entry(&advertised_head)],
                    continuation: None,
                    authority_records: authority.clone(),
                    device_logs: vec![vec![delta]],
                },
            );
            node.complete_next_device_writer_sync_work().await;
            assert_eq!(
                node.enumerate_append_records(&identity, "/load/items/")
                    .unwrap()
                    .len(),
                1
            );
        }
        assert_eq!(
            node.build_status()
                .device_writer_sync_work
                .verification_failed,
            2
        );
    }

    #[tokio::test]
    async fn two_peers_transfer_zero_then_one_entry_after_cold_sync() {
        async fn wait_for_listener(node: &mut NetworkNode) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while node.listeners().is_empty() {
                    let event = node.next_event().await;
                    node.handle_swarm_event(event);
                }
            })
            .await
            .expect("node did not start listening");
        }

        async fn complete_remote_sync(author: &mut NetworkNode, reader: &mut NetworkNode) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while reader.device_writer_sync_work.active.is_empty() {
                    tokio::select! {
                        event = author.next_event() => author.handle_swarm_event(event),
                        event = reader.next_event() => reader.handle_swarm_event(event),
                    }
                }
                reader.complete_next_device_writer_sync_work().await;
            })
            .await
            .expect("device-writer sync did not complete");
        }

        async fn wait_for_connection(
            author: &mut NetworkNode,
            reader: &mut NetworkNode,
            author_peer: libp2p::PeerId,
        ) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !reader.swarm.is_connected(&author_peer) {
                    tokio::select! {
                        event = author.next_event() => author.handle_swarm_event(event),
                        event = reader.next_event() => reader.handle_swarm_event(event),
                    }
                }
            })
            .await
            .expect("reader did not connect to author");
        }

        let author_dir = tempdir().unwrap();
        let reader_dir = tempdir().unwrap();
        let author_identity = NodeIdentity::generate();
        let identity = author_identity.identity_id();
        let mut author = NetworkNode::new_tcp(
            author_identity,
            make_store(author_dir.path()),
            NetworkConfig::test_config(),
        )
        .unwrap();
        let mut reader = make_node(reader_dir.path());
        let first_file = author_dir.path().join("first.json");
        std::fs::write(&first_file, br#"{"text":"first"}"#).unwrap();
        author
            .publish_file_appending_path(&first_file, "/load/items/first")
            .unwrap();

        author.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
        wait_for_listener(&mut author).await;
        let author_peer = *author.local_peer_id();
        let author_addr = author.listeners()[0]
            .clone()
            .with(libp2p::multiaddr::Protocol::P2p(author_peer));
        reader.listen_on("/ip4/127.0.0.1/tcp/0").unwrap();
        wait_for_listener(&mut reader).await;
        reader.dial(author_addr).unwrap();
        wait_for_connection(&mut author, &mut reader, author_peer).await;

        reader.request_device_writer_sync_from_provider(identity.clone(), &author_peer);
        complete_remote_sync(&mut author, &mut reader).await;
        let cold = reader.build_status().device_writer_sync_work;
        assert_eq!(cold.delta_responses, 1);
        assert_eq!(cold.received_entries, 1);

        reader.request_device_writer_sync_from_provider(identity.clone(), &author_peer);
        complete_remote_sync(&mut author, &mut reader).await;
        let unchanged = reader.build_status().device_writer_sync_work;
        assert_eq!(unchanged.delta_responses, 2);
        assert_eq!(
            unchanged.received_entries, 1,
            "an unchanged warm sync must transfer no writer entries"
        );

        let second_file = author_dir.path().join("second.json");
        std::fs::write(&second_file, br#"{"text":"second"}"#).unwrap();
        author
            .publish_file_appending_path(&second_file, "/load/items/second")
            .unwrap();
        reader.request_device_writer_sync_from_provider(identity.clone(), &author_peer);
        complete_remote_sync(&mut author, &mut reader).await;
        let appended = reader.build_status().device_writer_sync_work;
        assert_eq!(appended.delta_responses, 3);
        assert_eq!(
            appended.received_entries, 2,
            "the third sync must add exactly the one new writer entry"
        );
        assert_eq!(
            reader
                .enumerate_append_records(&identity, "/load/items/")
                .unwrap()
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn two_device_remote_identity_merges_deterministically_regardless_of_sync_order() {
        // Authorize two devices, each with its own append record, and verify the
        // merged enumeration is identical no matter which device log syncs
        // first. Each ordering is driven through a separate node.
        let root = NodeIdentity::generate();
        let phone = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        // Build a two-device authority chain: authorize phone, then laptop.
        let phone_auth = DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_phone",
                phone.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                1_780_579_200,
            ),
            1_780_579_200,
            |bytes| root.sign(bytes),
        )
        .unwrap();
        let laptop_auth = phone_auth
            .append(
                DeviceAuthorizationOperation::authorize_device(
                    "dev_laptop",
                    laptop.public_key_bytes(),
                    vec!["identity:write".to_string()],
                    Some("Laptop".to_string()),
                    1_780_579_300,
                ),
                1_780_579_300,
                |bytes| root.sign(bytes),
            )
            .unwrap();
        let two_device_authority = vec![phone_auth, laptop_auth];

        let from_phone = ContentId::from_bytes(b"post from phone");
        let from_laptop = ContentId::from_bytes(b"post from laptop");
        let phone_log = append_log(
            &identity,
            &phone,
            "dev_phone",
            &[("/spoke/posts/p", from_phone.clone())],
        );
        let laptop_log = append_log(
            &identity,
            &laptop,
            "dev_laptop",
            &[("/spoke/posts/l", from_laptop.clone())],
        );

        async fn enumerate_with_order(
            identity: jolt_core::IdentityId,
            authority: Vec<DeviceAuthorizationRecord>,
            first: Vec<DeviceWriterLogEntry>,
            second: Vec<DeviceWriterLogEntry>,
        ) -> Vec<(String, String)> {
            let dir = tempdir().unwrap();
            let mut node = make_node(dir.path());
            let provider = libp2p::PeerId::random();
            let key = NetworkNode::update_log_provider_key(&identity);
            node.discovered_providers.insert(key, vec![provider]);

            let (tx, rx) = oneshot::channel();
            node.handle_command(DaemonCommand::EnumerateAppendRecords {
                identity: identity.clone(),
                path_prefix: "/spoke/posts/".to_string(),
                response_tx: tx,
            });
            // The provider response carries both devices in a chosen order; the
            // merge is order-independent within the response too.
            deliver_device_writer_sync_response(
                &mut node,
                provider,
                authority,
                vec![first, second],
            )
            .await;
            rx.await
                .unwrap()
                .unwrap()
                .into_iter()
                .map(|record| (record.path, record.content_id))
                .collect::<Vec<_>>()
        }

        let phone_first = enumerate_with_order(
            identity.clone(),
            two_device_authority.clone(),
            phone_log.clone(),
            laptop_log.clone(),
        )
        .await;
        let laptop_first = enumerate_with_order(
            identity.clone(),
            two_device_authority,
            laptop_log,
            phone_log,
        )
        .await;

        assert_eq!(phone_first, laptop_first);
        assert_eq!(phone_first.len(), 2);
        assert!(phone_first
            .iter()
            .any(|(path, cid)| path == "/spoke/posts/p" && cid == &from_phone.to_string()));
        assert!(phone_first
            .iter()
            .any(|(path, cid)| path == "/spoke/posts/l" && cid == &from_laptop.to_string()));
    }

    #[tokio::test]
    async fn equal_length_device_log_forks_converge_regardless_of_arrival_order() {
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![authorize_device(&root, &device, "dev_laptop")];
        let first_fork = append_log(
            &identity,
            &device,
            "dev_laptop",
            &[("/spoke/posts/a", ContentId::from_bytes(b"fork a"))],
        );
        let second_fork = append_log(
            &identity,
            &device,
            "dev_laptop",
            &[("/spoke/posts/b", ContentId::from_bytes(b"fork b"))],
        );

        fn records_after(
            identity: jolt_core::IdentityId,
            authority: Vec<DeviceAuthorizationRecord>,
            first: Vec<DeviceWriterLogEntry>,
            second: Vec<DeviceWriterLogEntry>,
        ) -> Vec<(String, String)> {
            let dir = tempdir().unwrap();
            let mut node = make_node(dir.path());
            node.store_verified_device_writer_logs(
                identity.clone(),
                authority.clone(),
                vec![first],
            )
            .unwrap();
            node.store_verified_device_writer_logs(identity.clone(), authority, vec![second])
                .unwrap();
            node.device_writer_states[&identity]
                .merged
                .append_records_under("/spoke/posts/")
                .into_iter()
                .map(|(path, entry)| (path.to_string(), entry.content_id.to_string()))
                .collect()
        }

        let first_then_second = records_after(
            identity.clone(),
            authority.clone(),
            first_fork.clone(),
            second_fork.clone(),
        );
        let second_then_first = records_after(identity, authority, second_fork, first_fork);

        assert_eq!(first_then_second, second_then_first);
        assert_eq!(first_then_second.len(), 1);
    }

    #[tokio::test]
    async fn revoked_device_append_records_are_excluded_after_sync() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";

        // Authorize then revoke the device, accepting nothing it wrote.
        let authorize = DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                device_id,
                laptop.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Laptop".to_string()),
                1_780_579_200,
            ),
            1_780_579_200,
            |bytes| root.sign(bytes),
        )
        .unwrap();
        let revoke = authorize
            .append(
                DeviceAuthorizationOperation::revoke_device(
                    device_id,
                    None,
                    Some("lost device".to_string()),
                    1_780_579_400,
                ),
                1_780_579_400,
                |bytes| root.sign(bytes),
            )
            .unwrap();
        let authority = vec![authorize, revoke];

        let revoked_record = ContentId::from_bytes(b"record from revoked device");
        let device_log = append_log(
            &identity,
            &laptop,
            device_id,
            &[("/spoke/posts/1", revoked_record.clone())],
        );

        let provider = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        node.discovered_providers.insert(key, vec![provider]);

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });
        deliver_device_writer_sync_response(&mut node, provider, authority, vec![device_log]).await;

        let records = rx.await.unwrap().unwrap();
        assert!(
            records.is_empty(),
            "records from a revoked device must be excluded after sync, got {records:?}"
        );

        // The rejected entry is preserved as a diagnostic, not silently dropped.
        let state = node.device_writer_states.get(&identity).unwrap();
        assert!(state
            .merged
            .rejected_entries
            .iter()
            .any(|entry| entry.content_id.as_ref() == Some(&revoked_record)));

        drop(node);
        let reloaded = make_node(dir.path());
        assert!(reloaded
            .enumerate_append_records(&identity, "/spoke/posts/")
            .unwrap()
            .is_empty());
        assert!(reloaded.device_writer_states[&identity]
            .merged
            .rejected_entries
            .iter()
            .any(|entry| entry.content_id.as_ref() == Some(&revoked_record)));
    }

    #[tokio::test]
    async fn delta_authority_refresh_applies_revocation_and_rejects_rollback() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_revoked_delta";
        let authorize = authorize_device(&root, &device, device_id);
        let revoke = authorize
            .append(
                DeviceAuthorizationOperation::revoke_device(
                    device_id,
                    None,
                    Some("retired".to_string()),
                    200,
                ),
                200,
                |bytes| root.sign(bytes),
            )
            .unwrap();
        let log = large_append_log(&identity, &device, device_id, 1);
        node.store_verified_device_writer_logs(
            identity.clone(),
            vec![authorize.clone()],
            vec![log.clone()],
        )
        .unwrap();
        assert_eq!(
            node.enumerate_append_records(&identity, "/load/items/")
                .unwrap()
                .len(),
            1
        );
        let provider = libp2p::PeerId::random();

        for authority_records in [vec![authorize.clone(), revoke], vec![authorize.clone()]] {
            node.request_device_writer_sync_from_provider(identity.clone(), &provider);
            receive_device_writer_sync_page(
                &mut node,
                provider,
                DeviceWriterSyncResponse {
                    required_operation_version:
                        crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                    sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
                    heads: vec![DeviceWriterCursor::from_entry(log.last().unwrap())],
                    continuation: None,
                    authority_records,
                    device_logs: Vec::new(),
                },
            );
            node.complete_next_device_writer_sync_work().await;
            assert!(
                node.enumerate_append_records(&identity, "/load/items/")
                    .unwrap()
                    .is_empty(),
                "a stale authority response must not roll back the verified revocation"
            );
        }
    }

    #[tokio::test]
    async fn device_writer_sync_retries_next_provider_after_failure() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        let root = NodeIdentity::generate();
        let laptop = NodeIdentity::generate();
        let identity = root.identity_id();
        let device_id = "dev_laptop";
        let authority = vec![authorize_device(&root, &laptop, device_id)];
        let record = ContentId::from_bytes(b"record served by fallback provider");
        let device_log = append_log(
            &identity,
            &laptop,
            device_id,
            &[("/spoke/posts/1", record.clone())],
        );

        let failing = libp2p::PeerId::random();
        let fallback = libp2p::PeerId::random();
        let key = NetworkNode::update_log_provider_key(&identity);
        node.discovered_providers
            .insert(key, vec![failing, fallback]);

        let (tx, rx) = oneshot::channel();
        node.handle_command(DaemonCommand::EnumerateAppendRecords {
            identity: identity.clone(),
            path_prefix: "/spoke/posts/".to_string(),
            response_tx: tx,
        });

        let failing_request_id = *node.pending_device_writer_syncs.keys().next().unwrap();
        node.handle_swarm_event(SwarmEvent::Behaviour(JoltBehaviourEvent::DeviceWriterSync(
            request_response::Event::OutboundFailure {
                peer: failing,
                connection_id: libp2p::swarm::ConnectionId::new_unchecked(1),
                request_id: failing_request_id,
                error: request_response::OutboundFailure::DialFailure,
            },
        )));

        // The sync should now be retrying the fallback provider.
        assert_eq!(node.pending_device_writer_syncs.len(), 1);
        let retry = node.pending_device_writer_syncs.values().next().unwrap();
        assert_eq!(retry.provider, fallback);

        deliver_device_writer_sync_response(&mut node, fallback, authority, vec![device_log]).await;
        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].content_id, record.to_string());
    }

    #[tokio::test]
    async fn daemon_resolution_times_out_when_update_log_provider_stalls() {
        let dir = tempdir().unwrap();
        let mut node = make_node(dir.path());
        node.set_resolve_timeout(Duration::from_millis(0));
        let owner = NodeIdentity::generate();
        let address = JoltAddress::new(owner.identity_id(), "/profile").unwrap();
        let provider = libp2p::PeerId::random();

        let (tx, rx) = oneshot::channel();
        node.request_daemon_resolve_from_provider(address, None, &provider, tx, None);

        assert_eq!(node.pending_daemon_resolutions.len(), 1);
        node.check_resolve_timeouts();

        assert_eq!(node.pending_daemon_resolutions.len(), 0);
        assert!(matches!(rx.await.unwrap(), Err(NetworkError::Timeout)));
    }
}
