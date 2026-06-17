use std::time::{Duration, Instant};

use jolt_core::{
    merge_device_writer_logs, resolve_jolt_address, resolve_merged_device_jolt_address,
    verify_identity_authority_chain, verify_update_log_for_identity, DeviceAuthorizationRecord,
    DeviceWriterLogEntry, IdentityId, JoltAddress, ResolvedJoltTarget,
};
use tokio::sync::oneshot;

use crate::command::{AppendRecordInfo, ResolveResponse};
use crate::error::NetworkError;
use crate::protocol::UpdateLogRequest;

use super::CachedDeviceWriterState;
use super::NetworkNode;

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
                content_id: entry.content_id.to_string(),
                device_id: entry.device_id.clone(),
                device_sequence: entry.device_sequence,
                created_at: entry.created_at,
                entry_hash: entry.entry_hash.to_hex(),
            })
            .collect())
    }

    /// Store a verified merged device-writer state for an identity.
    pub fn store_verified_device_writer_logs(
        &mut self,
        identity: IdentityId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Result<u64, NetworkError> {
        let authority = verify_identity_authority_chain(&identity, &authority_records)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let merged = merge_device_writer_logs(&authority, device_logs)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let authority_sequence = authority.latest_sequence;
        self.device_writer_states.insert(
            identity,
            CachedDeviceWriterState {
                authority_sequence,
                merged,
            },
        );
        Ok(authority_sequence)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
    use crate::protocol::UpdateLogResponse;

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
