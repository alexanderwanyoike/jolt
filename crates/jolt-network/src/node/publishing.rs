use std::collections::HashMap;
use std::path::Path;

use libp2p::multiaddr::Protocol;
use tracing::debug;

use jolt_core::{
    verify_update_log_for_identity, ContentId, ContentManifest, DeviceAuthorizationOperation,
    DeviceAuthorizationRecord, DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
    IdentityHeadHint, IdentityId, JoltAddress, LiveReachabilityEndpoint, OfflineIngressEndpoint,
    ReachabilityRecord, UpdateAction, UpdateLogEntry, VerifiedReachability,
    SIGNED_REACHABILITY_PATH,
};
use jolt_identity::NodeIdentity;
use jolt_store::{
    ContentStore, PersistedDeviceWriterLog, PersistedRecordMutation,
    PersistedRecordMutationOperation,
};

use crate::command::{
    LocalRecordDelete, LocalRecordRestore, LocalRecordUpdate, PublishReachabilityResponse,
};
use crate::error::NetworkError;

use super::{unix_now, NetworkNode, RELAY_RECORD_TTL_SECS};

const LEGACY_ROOT_DEVICE_ID: &str = "dev_legacy_root";

struct RecordMutationIntent<'a> {
    mutation_id: &'a str,
    observed_revision: &'a str,
    operation: PersistedRecordMutationOperation,
    content_id: Option<&'a ContentId>,
}

fn validate_record_mutation_id(mutation_id: &str) -> Result<(), NetworkError> {
    if mutation_id.is_empty() || mutation_id.len() > 256 {
        return Err(NetworkError::InvalidInput(
            "mutation_id must contain 1 to 256 bytes".to_string(),
        ));
    }
    Ok(())
}

impl NetworkNode {
    /// Publish a file to the content store. Returns the ContentId.
    pub fn publish_file(&mut self, file_path: &Path) -> Result<ContentId, NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
        self.publish_bytes(&data)
    }

    pub(super) fn publish_bytes(&mut self, data: &[u8]) -> Result<ContentId, NetworkError> {
        let content_id = ContentId::from_bytes(&data);

        let signature = self.identity.sign(&data);

        let manifest = ContentManifest {
            content_id: content_id.clone(),
            size: data.len() as u64,
            content_type: "application/octet-stream".to_string(),
            publisher_key: self.identity.public_key_bytes().to_vec(),
            signature,
        };

        self.store
            .publish(&data, &manifest)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        // Announce as DHT provider.
        if let Err(e) = self.announce_provider(&content_id) {
            debug!("DHT announcement skipped: {e}");
        }

        Ok(content_id)
    }

    /// Publish a file and bind the resulting CID to an opaque path in this
    /// node's signed identity namespace.
    pub fn publish_file_at_path(
        &mut self,
        file_path: &Path,
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64, String), NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
        self.publish_bytes_at_path(&data, path)
    }

    pub(super) fn publish_bytes_at_path(
        &mut self,
        data: &[u8],
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64, String), NetworkError> {
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_id = self.publish_bytes(data)?;
        let action = UpdateAction::SetPath {
            path: address.path().to_string(),
            content_id: content_id.clone(),
        };
        let latest_sequence = self.publish_local_update_log_action(&identity, action)?;
        let (_, revision) = self.publish_local_device_writer_path(
            identity.clone(),
            address.path().to_string(),
            content_id.clone(),
            DeviceWriterPathMode::Singleton,
            None,
        )?;

        Ok((content_id, address, latest_sequence, revision))
    }

    fn publish_local_update_log_action(
        &mut self,
        identity: &IdentityId,
        action: UpdateAction,
    ) -> Result<u64, NetworkError> {
        let entry = match self
            .update_logs
            .get(identity)
            .and_then(|entries| entries.last())
        {
            Some(previous) => previous
                .append(action, |bytes| self.identity.sign(bytes))
                .map_err(|e| NetworkError::Protocol(e.to_string()))?,
            None => UpdateLogEntry::genesis(self.identity.public_key_bytes(), action, |bytes| {
                self.identity.sign(bytes)
            })
            .map_err(|e| NetworkError::Protocol(e.to_string()))?,
        };
        let latest_sequence = entry.body.sequence;
        let entries_to_save = {
            let entries = self.update_logs.entry(identity.clone()).or_default();
            entries.push(entry);
            entries.clone()
        };
        self.store
            .save_update_log(identity, &entries_to_save)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        if let Err(e) = self.announce_update_log_provider(identity) {
            debug!("Update-log DHT announcement skipped: {e}");
        }
        if let Err(e) = self.refresh_local_identity_head_hint(identity) {
            debug!("Identity-head hint refresh skipped: {e}");
        }
        Ok(latest_sequence)
    }

    pub(super) fn update_local_record(
        &mut self,
        path: &str,
        data: &[u8],
        observed_revision: &str,
        mutation_id: &str,
    ) -> Result<LocalRecordUpdate, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let proposed_content_id = ContentId::from_bytes(data).to_string();
        if let Some(previous) = self
            .local_record_mutations
            .get(&identity)
            .and_then(|mutations| mutations.get(mutation_id))
        {
            let Some(content_id) = previous.content_id.as_ref() else {
                return Err(NetworkError::InvalidInput(
                    "mutation_id was already used for a different record mutation".to_string(),
                ));
            };
            if previous.operation() != PersistedRecordMutationOperation::Update
                || previous.path != address.path()
                || previous.observed_revision != observed_revision
                || content_id != &proposed_content_id
            {
                return Err(NetworkError::InvalidInput(
                    "mutation_id was already used for a different record mutation".to_string(),
                ));
            }
            let stored = self
                .store
                .get_content(content_id)
                .ok_or_else(|| NetworkError::ContentNotFound(content_id.clone()))?;
            return Ok(LocalRecordUpdate {
                path: previous.path.clone(),
                content_id: content_id.clone(),
                revision: previous.result_revision.clone(),
                data: stored.data,
            });
        }

        let crate::command::LocalRecordState::Present(current) =
            self.inspect_local_record(address.path())
        else {
            return Err(NetworkError::RecordConflict);
        };
        if current.revision != observed_revision {
            return Err(NetworkError::RecordConflict);
        }

        // Content is immutable and may safely remain unreferenced if the
        // subsequent durable log append fails.
        let content_id = self.publish_bytes(data)?;
        self.publish_local_update_log_action(
            &identity,
            UpdateAction::SetPath {
                path: address.path().to_string(),
                content_id: content_id.clone(),
            },
        )?;
        let (_, result_revision) = self.publish_local_device_writer_path(
            identity,
            address.path().to_string(),
            content_id.clone(),
            DeviceWriterPathMode::Singleton,
            Some(RecordMutationIntent {
                mutation_id,
                observed_revision,
                operation: PersistedRecordMutationOperation::Update,
                content_id: Some(&content_id),
            }),
        )?;

        Ok(LocalRecordUpdate {
            path: address.path().to_string(),
            content_id: content_id.to_string(),
            revision: result_revision,
            data: data.to_vec(),
        })
    }

    pub(super) fn delete_local_record(
        &mut self,
        path: &str,
        observed_revision: &str,
        mutation_id: &str,
    ) -> Result<LocalRecordDelete, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        if let Some(previous) = self
            .local_record_mutations
            .get(&identity)
            .and_then(|mutations| mutations.get(mutation_id))
        {
            if previous.operation() != PersistedRecordMutationOperation::Delete
                || previous.path != address.path()
                || previous.observed_revision != observed_revision
                || previous.content_id.is_some()
            {
                return Err(NetworkError::InvalidInput(
                    "mutation_id was already used for a different record mutation".to_string(),
                ));
            }
            return Ok(LocalRecordDelete {
                path: previous.path.clone(),
                revision: previous.result_revision.clone(),
            });
        }

        let crate::command::LocalRecordState::Present(current) =
            self.inspect_local_record(address.path())
        else {
            return Err(NetworkError::RecordConflict);
        };
        if current.revision != observed_revision {
            return Err(NetworkError::RecordConflict);
        }

        self.publish_local_update_log_action(
            &identity,
            UpdateAction::RemovePath {
                path: address.path().to_string(),
            },
        )?;
        let (_, result_revision) = self.publish_local_device_writer_operation(
            identity,
            address.path().to_string(),
            DeviceWriterOperation::tombstone_path(address.path()),
            Some(RecordMutationIntent {
                mutation_id,
                observed_revision,
                operation: PersistedRecordMutationOperation::Delete,
                content_id: None,
            }),
        )?;
        Ok(LocalRecordDelete {
            path: address.path().to_string(),
            revision: result_revision,
        })
    }

    pub(super) fn restore_local_record(
        &mut self,
        path: &str,
        data: &[u8],
        observed_revision: &str,
        mutation_id: &str,
    ) -> Result<LocalRecordRestore, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let proposed_content_id = ContentId::from_bytes(data).to_string();
        if let Some(previous) = self
            .local_record_mutations
            .get(&identity)
            .and_then(|mutations| mutations.get(mutation_id))
        {
            let Some(content_id) = previous.content_id.as_ref() else {
                return Err(NetworkError::InvalidInput(
                    "mutation_id was already used for a different record mutation".to_string(),
                ));
            };
            if previous.operation() != PersistedRecordMutationOperation::Restore
                || previous.path != address.path()
                || previous.observed_revision != observed_revision
                || content_id != &proposed_content_id
            {
                return Err(NetworkError::InvalidInput(
                    "mutation_id was already used for a different record mutation".to_string(),
                ));
            }
            let stored = self
                .store
                .get_content(content_id)
                .ok_or_else(|| NetworkError::ContentNotFound(content_id.clone()))?;
            return Ok(LocalRecordRestore {
                path: previous.path.clone(),
                content_id: content_id.clone(),
                revision: previous.result_revision.clone(),
                data: stored.data,
            });
        }

        let crate::command::LocalRecordState::Deleted { revision, .. } =
            self.inspect_local_record(address.path())
        else {
            return Err(NetworkError::RecordConflict);
        };
        if revision != observed_revision {
            return Err(NetworkError::RecordConflict);
        }

        let content_id = self.publish_bytes(data)?;
        self.publish_local_update_log_action(
            &identity,
            UpdateAction::SetPath {
                path: address.path().to_string(),
                content_id: content_id.clone(),
            },
        )?;
        let (_, result_revision) = self.publish_local_device_writer_path(
            identity,
            address.path().to_string(),
            content_id.clone(),
            DeviceWriterPathMode::Singleton,
            Some(RecordMutationIntent {
                mutation_id,
                observed_revision,
                operation: PersistedRecordMutationOperation::Restore,
                content_id: Some(&content_id),
            }),
        )?;

        Ok(LocalRecordRestore {
            path: address.path().to_string(),
            content_id: content_id.to_string(),
            revision: result_revision,
            data: data.to_vec(),
        })
    }

    /// Publish a file as an append record bound to a path in this node's signed
    /// identity namespace. Unlike `publish_file_at_path`, this never writes the
    /// last-writer-wins update log: append records are independent elements of a
    /// growing collection and must all coexist.
    pub fn publish_file_appending_path(
        &mut self,
        file_path: &Path,
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64), NetworkError> {
        let data = std::fs::read(file_path).map_err(NetworkError::Io)?;
        self.publish_bytes_appending_path(&data, path)
    }

    pub(super) fn publish_bytes_appending_path(
        &mut self,
        data: &[u8],
        path: &str,
    ) -> Result<(ContentId, JoltAddress, u64), NetworkError> {
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_id = self.publish_bytes(data)?;
        let (device_sequence, _) = self.publish_local_device_writer_path(
            identity,
            address.path().to_string(),
            content_id.clone(),
            DeviceWriterPathMode::Append,
            None,
        )?;
        Ok((content_id, address, device_sequence))
    }

    fn publish_local_device_writer_path(
        &mut self,
        identity: IdentityId,
        path: String,
        content_id: ContentId,
        mode: DeviceWriterPathMode,
        record_mutation: Option<RecordMutationIntent<'_>>,
    ) -> Result<(u64, String), NetworkError> {
        let operation = DeviceWriterOperation::set_path(path.clone(), content_id.clone(), mode);
        self.publish_local_device_writer_operation(identity, path, operation, record_mutation)
    }

    fn publish_local_device_writer_operation(
        &mut self,
        identity: IdentityId,
        path: String,
        operation: DeviceWriterOperation,
        record_mutation: Option<RecordMutationIntent<'_>>,
    ) -> Result<(u64, String), NetworkError> {
        let created_at = unix_now();
        let entry = match self
            .local_device_writer_logs
            .get(&identity)
            .and_then(|entries| entries.last())
        {
            Some(previous) => previous
                .append(operation, created_at, |bytes| self.identity.sign(bytes))
                .map_err(|e| NetworkError::Protocol(e.to_string()))?,
            None => DeviceWriterLogEntry::genesis(
                identity.clone(),
                LEGACY_ROOT_DEVICE_ID,
                operation,
                created_at,
                |bytes| self.identity.sign(bytes),
            )
            .map_err(|e| NetworkError::Protocol(e.to_string()))?,
        };

        let authority_records = match self.local_device_authority_records.get(&identity) {
            Some(records) => records.clone(),
            None => vec![DeviceAuthorizationRecord::genesis(
                self.identity.public_key_bytes(),
                identity.clone(),
                DeviceAuthorizationOperation::authorize_device(
                    LEGACY_ROOT_DEVICE_ID,
                    self.identity.public_key_bytes(),
                    vec!["identity:write".to_string()],
                    Some("Legacy root device".to_string()),
                    created_at,
                ),
                created_at,
                |bytes| self.identity.sign(bytes),
            )
            .map_err(|e| NetworkError::Protocol(e.to_string()))?],
        };
        let device_sequence = entry.body.device_sequence;
        let revision = entry.entry_hash().to_hex();
        let mut device_log = self
            .local_device_writer_logs
            .get(&identity)
            .cloned()
            .unwrap_or_default();
        device_log.push(entry);
        let mut record_mutations = self
            .local_record_mutations
            .get(&identity)
            .cloned()
            .unwrap_or_default();
        if let Some(record_mutation) = record_mutation {
            record_mutations.insert(
                record_mutation.mutation_id.to_string(),
                PersistedRecordMutation {
                    path,
                    observed_revision: record_mutation.observed_revision.to_string(),
                    operation: Some(record_mutation.operation),
                    content_id: record_mutation.content_id.map(ToString::to_string),
                    result_revision: revision.clone(),
                },
            );
        }

        // Persist the device-writer log before serving it. Append records live
        // only here (never the update log), so without this they would not
        // survive a daemon restart - the identity's own posts/accepted-reply
        // refs, and the records it serves to peers, would vanish.
        self.store
            .save_device_writer_log(
                &identity,
                &PersistedDeviceWriterLog {
                    authority_records: authority_records.clone(),
                    device_log: device_log.clone(),
                    record_mutations: record_mutations.clone(),
                },
            )
            .map_err(|e| {
                NetworkError::Protocol(format!(
                    "failed to persist device-writer log for {identity}: {e}"
                ))
            })?;

        self.store_verified_device_writer_logs(
            identity.clone(),
            authority_records.clone(),
            vec![device_log.clone()],
        )?;
        self.local_device_authority_records
            .insert(identity.clone(), authority_records);
        self.local_device_writer_logs
            .insert(identity.clone(), device_log);
        self.local_record_mutations
            .insert(identity.clone(), record_mutations);

        // Announce this node as a provider for the identity so remote readers
        // can discover and sync the device-writer logs. Reuses the existing
        // update-log provider key, so device-writer sync rides the same
        // discovery path. Append-only publishes never touch the update log, so
        // without this they would otherwise be undiscoverable.
        if let Err(e) = self.announce_update_log_provider(&identity) {
            debug!("Device-writer provider announcement skipped: {e}");
        }
        Ok((device_sequence, revision))
    }

    pub(super) fn publish_reachability(
        &mut self,
        sequence_hint: u64,
        expires_at: u64,
        live: Vec<LiveReachabilityEndpoint>,
        offline_ingress: Vec<OfflineIngressEndpoint>,
    ) -> Result<PublishReachabilityResponse, NetworkError> {
        let now = unix_now();
        let identity = self.identity.identity_id();
        let record = ReachabilityRecord::new(
            self.identity.public_key_bytes(),
            identity.clone(),
            sequence_hint,
            now,
            expires_at,
            live,
            offline_ingress,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let record_bytes =
            serde_json::to_vec(&record).map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let (content_id, address, latest_sequence, _) =
            self.publish_bytes_at_path(&record_bytes, SIGNED_REACHABILITY_PATH)?;
        let record = VerifiedReachability {
            identity: identity.clone(),
            sequence_hint,
            expires_at,
            live: record.body.live,
            offline_ingress: record.body.offline_ingress,
        };

        Ok(PublishReachabilityResponse {
            identity: identity.to_string(),
            path: SIGNED_REACHABILITY_PATH.to_string(),
            address: address.to_string(),
            latest_sequence,
            content_id: content_id.to_string(),
            record,
        })
    }

    pub(super) fn publish_update_log_snapshot(
        &mut self,
        identity: &IdentityId,
    ) -> Result<Option<ContentId>, NetworkError> {
        let Some(entries) = self.update_logs.get(identity).cloned() else {
            return Ok(None);
        };
        verify_update_log_for_identity(identity, &entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        let data =
            serde_json::to_vec(&entries).map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let content_id = ContentId::from_bytes(&data);
        let signature = self.identity.sign(&data);
        let manifest = ContentManifest {
            content_id: content_id.clone(),
            size: data.len() as u64,
            content_type: "application/jolt-update-log+json".to_string(),
            publisher_key: self.identity.public_key_bytes().to_vec(),
            signature,
        };

        self.store
            .publish(&data, &manifest)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;

        if let Err(e) = self.announce_provider(&content_id) {
            debug!("Update-log snapshot DHT announcement skipped: {e}");
        }

        Ok(Some(content_id))
    }

    fn refresh_local_identity_head_hint(
        &mut self,
        identity: &IdentityId,
    ) -> Result<(), NetworkError> {
        if identity != &self.identity.identity_id() {
            return Ok(());
        }

        let Some(entries) = self.update_logs.get(identity) else {
            return Ok(());
        };
        let Some(latest) = entries.last() else {
            return Ok(());
        };

        let now = unix_now();
        let provider_addrs = self
            .swarm
            .listeners()
            .map(|addr| {
                if addr
                    .iter()
                    .any(|protocol| matches!(protocol, Protocol::P2p(_)))
                {
                    addr.to_string()
                } else {
                    addr.clone()
                        .with(Protocol::P2p(*self.swarm.local_peer_id()))
                        .to_string()
                }
            })
            .collect::<Vec<_>>();
        if provider_addrs.is_empty() {
            return Ok(());
        }

        let hint = IdentityHeadHint::new(
            self.identity.public_key_bytes(),
            identity.clone(),
            self.swarm.local_peer_id().to_string(),
            provider_addrs,
            None,
            latest.body.sequence,
            latest.entry_hash(),
            now,
            now + RELAY_RECORD_TTL_SECS,
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        self.record_identity_head_hint(hint)
    }

    pub(super) fn load_persisted_local_update_log(
        store: &ContentStore,
        identity: &NodeIdentity,
    ) -> Result<HashMap<IdentityId, Vec<UpdateLogEntry>>, NetworkError> {
        let identity_id = identity.identity_id();
        let Some(entries) = store.load_update_log(&identity_id).map_err(|e| {
            NetworkError::Protocol(format!(
                "failed to load persisted update log for {identity_id}: {e}"
            ))
        })?
        else {
            return Ok(HashMap::new());
        };

        verify_update_log_for_identity(&identity_id, &entries).map_err(|e| {
            NetworkError::Protocol(format!(
                "invalid persisted update log for {identity_id}: {e}"
            ))
        })?;

        let mut update_logs = HashMap::new();
        update_logs.insert(identity_id, entries);
        Ok(update_logs)
    }

    /// Rebuild this node's own device-writer state from the persisted log, if
    /// any. Repopulates the local device log and authority records (so a later
    /// append continues the existing per-device chain) and the merged
    /// device-writer state (so the identity's append records enumerate and are
    /// served to peers) - the in-memory counterpart of `save_device_writer_log`.
    pub(super) fn load_persisted_local_device_writer_log(&mut self) -> Result<(), NetworkError> {
        let identity_id = self.identity.identity_id();
        let Some(record) = self
            .store
            .load_device_writer_log(&identity_id)
            .map_err(|e| {
                NetworkError::Protocol(format!(
                    "failed to load persisted device-writer log for {identity_id}: {e}"
                ))
            })?
        else {
            return Ok(());
        };

        // store_verified_device_writer_logs verifies the authority chain and
        // merges the log into device_writer_states; reject a tampered log just
        // as the persisted update-log path does.
        self.store_verified_device_writer_logs(
            identity_id.clone(),
            record.authority_records.clone(),
            vec![record.device_log.clone()],
        )
        .map_err(|e| {
            NetworkError::Protocol(format!(
                "invalid persisted device-writer log for {identity_id}: {e}"
            ))
        })?;

        self.local_device_authority_records
            .insert(identity_id.clone(), record.authority_records);
        self.local_device_writer_logs
            .insert(identity_id.clone(), record.device_log);
        self.local_record_mutations
            .insert(identity_id, record.record_mutations);
        Ok(())
    }

    /// Store a verified update log for an identity, ignoring stale valid logs.
    pub fn store_verified_update_log(
        &mut self,
        identity: IdentityId,
        entries: Vec<UpdateLogEntry>,
    ) -> Result<(), NetworkError> {
        let candidate_sequence = verify_update_log_for_identity(&identity, &entries)
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let current_sequence = self
            .update_logs
            .get(&identity)
            .and_then(|current| verify_update_log_for_identity(&identity, current).ok());

        if current_sequence
            .map(|current| candidate_sequence > current)
            .unwrap_or(true)
        {
            self.update_logs.insert(identity.clone(), entries);
            if let Err(e) = self.refresh_local_identity_head_hint(&identity) {
                debug!("Identity-head hint refresh skipped: {e}");
            }
        }

        Ok(())
    }

    /// Return the verified update log entries this node knows for an identity.
    pub fn update_log_entries(&self, identity: &IdentityId) -> Option<&[UpdateLogEntry]> {
        self.update_logs.get(identity).map(Vec::as_slice)
    }
}
