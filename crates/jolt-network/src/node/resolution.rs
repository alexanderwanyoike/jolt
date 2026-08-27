use std::collections::HashMap;
use std::time::{Duration, Instant};

use jolt_core::{
    merge_device_writer_logs, resolve_jolt_address, resolve_merged_device_jolt_address,
    verify_identity_authority_chain, verify_update_log_for_identity, DeviceAuthorizationRecord,
    DeviceWriterLogEntry, IdentityId, JoltAddress, ResolvedJoltTarget,
};
use tokio::sync::oneshot;

use crate::command::{AppendRecordInfo, ResolveResponse};
use crate::error::NetworkError;
use crate::protocol::{DeviceWriterSyncRequest, UpdateLogRequest};

use super::CachedDeviceWriterState;
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
    /// A background refresh that only populates the device-writer cache so that
    /// later enumerations/resolutions see live remote state. No waiter is
    /// answered. This is how a `.jolt` resolve opportunistically warms the
    /// device-writer cache while the legacy update-log path answers the resolve.
    Refresh { identity: IdentityId },
}

impl DeviceWriterSyncWaiter {
    fn identity(&self) -> &IdentityId {
        match self {
            Self::Enumerate { identity, .. } => identity,
            Self::Refresh { identity } => identity,
        }
    }
}

/// An in-flight device-writer sync request to a single provider.
pub(super) struct PendingDeviceWriterSync {
    pub(super) identity: IdentityId,
    pub(super) provider: libp2p::PeerId,
    pub(super) deadline: Instant,
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
        let target = resolve_merged_device_jolt_address(address, &state.merged)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

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
                content_id: entry
                    .content_id()
                    .expect("append records are always content-bearing")
                    .to_string(),
                device_id: entry.device_id.clone(),
                device_sequence: entry.device_sequence,
                created_at: entry.created_at,
                entry_hash: entry.entry_hash.to_hex(),
            })
            .collect())
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
        let candidate_authority = verify_identity_authority_chain(&identity, &authority_records)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        // Choose the authority chain to merge under: keep whichever verified
        // chain has the higher sequence so a revocation is never dropped.
        let existing = self.device_writer_states.get(&identity);
        let use_candidate_authority = existing
            .map(|state| candidate_authority.latest_sequence >= state.authority_sequence)
            .unwrap_or(true);
        let authority_records = if use_candidate_authority {
            authority_records
        } else {
            existing
                .map(|state| state.authority_records.clone())
                .unwrap_or(authority_records)
        };
        let authority = verify_identity_authority_chain(&identity, &authority_records)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        // Accumulate device logs by device id. The longest log per device wins.
        // Equal-length forks use the same terminal-entry ordering tuple as the
        // core merge so provider arrival order cannot select the fork.
        let mut accumulated: HashMap<String, Vec<DeviceWriterLogEntry>> = existing
            .map(|state| state.device_logs.clone())
            .unwrap_or_default();
        for log in device_logs {
            let Some(first) = log.first() else {
                continue;
            };
            let device_id = first.body.device_id.clone();
            let keep = accumulated
                .get(&device_id)
                .map(|known| device_log_is_newer(&log, known))
                .unwrap_or(true);
            if keep {
                accumulated.insert(device_id, log);
            }
        }

        let merged = merge_device_writer_logs(&authority, accumulated.values().cloned())
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let authority_sequence = authority.latest_sequence;
        self.device_writer_states.insert(
            identity,
            CachedDeviceWriterState {
                authority_sequence,
                merged,
                authority_records,
                device_logs: accumulated,
            },
        );
        Ok(authority_sequence)
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
        let is_background_refresh = matches!(&waiter, DeviceWriterSyncWaiter::Refresh { .. });

        // Explicit enumerations wait for an in-flight sync. Background refresh
        // waiters carry no response, so accumulating one per cache hit only
        // wastes memory and work.
        let sync_in_flight = self
            .pending_device_writer_syncs
            .values()
            .any(|pending| pending.identity == identity);
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

        if is_background_refresh && !self.mark_device_writer_refresh_if_due(&identity) {
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
            self.answer_device_writer_waiters(&identity);
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
        let request = DeviceWriterSyncRequest::new(identity.clone());
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
                deadline: Instant::now() + self.resolve_timeout,
            },
        );
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
                DeviceWriterSyncWaiter::Refresh { .. } => {}
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
            }
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
        self.answer_device_writer_waiters(identity);
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
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
    use crate::protocol::{DeviceWriterSyncResponse, UpdateLogResponse};

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

    /// Drive a device-writer sync triggered by a daemon command: feed the
    /// in-flight `device_writer_sync` request a provider response carrying the
    /// supplied authority records and device logs.
    fn deliver_device_writer_sync_response(
        node: &mut NetworkNode,
        provider: libp2p::PeerId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
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
                    response: DeviceWriterSyncResponse {
                        required_operation_version:
                            crate::protocol::LEGACY_DEVICE_WRITER_OPERATION_VERSION,
                        authority_records,
                        device_logs,
                    },
                },
            },
        )));
    }

    fn make_store(dir: &std::path::Path) -> ContentStore {
        ContentStore::open(dir, CacheConfig::default()).unwrap()
    }

    fn make_node(dir: &std::path::Path) -> NetworkNode {
        let identity = NodeIdentity::generate();
        let store = make_store(dir);
        NetworkNode::new_tcp(identity, store, NetworkConfig::test_config()).unwrap()
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

        deliver_device_writer_sync_response(&mut node, provider, Vec::new(), Vec::new());
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

        assert!(NetworkNode::mark_refresh_if_due(
            &mut refreshes,
            &identity
        ));
        assert!(!NetworkNode::mark_refresh_if_due(
            &mut refreshes,
            &identity
        ));

        refreshes.insert(
            identity.clone(),
            Instant::now() - super::super::CACHED_IDENTITY_REFRESH_INTERVAL,
        );
        assert!(NetworkNode::mark_refresh_if_due(
            &mut refreshes,
            &identity
        ));
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

        deliver_device_writer_sync_response(&mut node, provider, authority, vec![device_log]);

        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].path, "/spoke/posts/1");
        assert_eq!(records[0].content_id, post_a.to_string());
        assert_eq!(records[1].path, "/spoke/posts/2");
        assert_eq!(records[1].content_id, post_b.to_string());
        assert_eq!(records[0].device_id, device_id);
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
        );
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
        deliver_device_writer_sync_response(&mut node, provider, authority, vec![second_log]);

        let records = rx.await.unwrap().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].path, "/spoke/accepted/post_1/reply_1");
        assert_eq!(records[0].content_id, accepted.to_string());
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
            );
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
                .map(|(path, entry)| {
                    (
                        path.to_string(),
                        entry
                            .content_id()
                            .expect("append records are always content-bearing")
                            .to_string(),
                    )
                })
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
        deliver_device_writer_sync_response(&mut node, provider, authority, vec![device_log]);

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

        deliver_device_writer_sync_response(&mut node, fallback, authority, vec![device_log]);
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
