use std::collections::HashMap;
use std::path::Path;

use libp2p::multiaddr::Protocol;
use tracing::debug;

use jolt_core::{
    verify_identity_authority_chain, verify_update_log_for_identity, AuthorizedDeviceStatus,
    ContentId, ContentManifest, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
    DeviceWriterLogEntry, DeviceWriterLogEntryHash, DeviceWriterOperation, DeviceWriterPathMode,
    IdentityHeadHint, IdentityId, JoltAddress, LiveReachabilityEndpoint, OfflineIngressEndpoint,
    ReachabilityRecord, UpdateAction, UpdateLogEntry, VerifiedReachability,
    IDENTITY_AUTHORITY_PATH, SIGNED_REACHABILITY_PATH,
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

struct RecordMutationIntent<'a> {
    mutation_id: &'a str,
    observed_revisions: &'a [String],
    operation: PersistedRecordMutationOperation,
    content_id: Option<&'a ContentId>,
}

#[derive(Clone, Copy)]
enum RequiredSingleRecordState {
    Present,
    Deleted,
}

fn local_record_head_revision(head: &crate::command::LocalRecordHead) -> &str {
    match head {
        crate::command::LocalRecordHead::Deleted { revision } => revision,
        crate::command::LocalRecordHead::Present(record) => &record.revision,
    }
}

fn requested_record_revisions(
    revision: &str,
    observed_revisions: &[String],
) -> Result<Vec<String>, NetworkError> {
    if observed_revisions.is_empty() {
        return Ok(vec![revision.to_string()]);
    }
    if observed_revisions.last().map(String::as_str) != Some(revision) {
        return Err(NetworkError::InvalidInput(
            "revision must name the final observed_revisions head".to_string(),
        ));
    }
    Ok(observed_revisions.to_vec())
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
    fn validate_current_record_revisions(
        &self,
        path: &str,
        requested_revisions: &[String],
        required_single_state: RequiredSingleRecordState,
    ) -> Result<(), NetworkError> {
        let current_revisions = match self.inspect_local_record(path) {
            crate::command::LocalRecordState::Present(current)
                if requested_revisions.len() == 1
                    && matches!(required_single_state, RequiredSingleRecordState::Present) =>
            {
                vec![current.revision]
            }
            crate::command::LocalRecordState::Deleted { revision, .. }
                if requested_revisions.len() == 1
                    && matches!(required_single_state, RequiredSingleRecordState::Deleted) =>
            {
                vec![revision]
            }
            crate::command::LocalRecordState::Conflicted { alternatives, .. }
                if requested_revisions.len() > 1 =>
            {
                alternatives
                    .iter()
                    .map(local_record_head_revision)
                    .map(ToString::to_string)
                    .collect()
            }
            _ => return Err(NetworkError::RecordConflict),
        };
        if current_revisions != requested_revisions {
            return Err(NetworkError::RecordConflict);
        }
        Ok(())
    }

    /// The heads a plain singleton write must supersede: the path's current
    /// winner and any concurrent candidates. A write that observes nothing is
    /// merely concurrent with them, and the device-sequence tie-break then
    /// lets an older, longer-lived device keep winning: the legacy root
    /// device's 1 September reachability record beat every renewed one.
    fn current_singleton_heads(
        &self,
        identity: &IdentityId,
        operation: &DeviceWriterOperation,
    ) -> Vec<DeviceWriterLogEntryHash> {
        let path = match operation {
            DeviceWriterOperation::SetPath {
                path,
                mode: DeviceWriterPathMode::Singleton,
                ..
            } => path,
            DeviceWriterOperation::TombstonePath { path } => path,
            DeviceWriterOperation::SetPath { .. } => return Vec::new(),
        };
        let Some(state) = self.device_writer_states.get(identity) else {
            return Vec::new();
        };
        let mut heads: Vec<DeviceWriterLogEntryHash> = state
            .merged
            .singleton_paths
            .get(path)
            .map(|entry| entry.entry_hash.clone())
            .into_iter()
            .collect();
        if let Some(conflicts) = state.merged.singleton_conflicts.get(path) {
            heads.extend(conflicts.iter().map(|entry| entry.entry_hash.clone()));
        }
        heads
    }

    fn observed_record_head_hashes(
        &self,
        identity: &IdentityId,
        revisions: &[String],
    ) -> Result<Vec<DeviceWriterLogEntryHash>, NetworkError> {
        let Some(state) = self.device_writer_states.get(identity) else {
            return Err(NetworkError::RecordConflict);
        };
        revisions
            .iter()
            .map(|revision| {
                state
                    .device_logs
                    .values()
                    .flatten()
                    .find_map(|entry| {
                        let hash = entry.entry_hash();
                        (hash.to_hex() == *revision).then_some(hash)
                    })
                    .ok_or(NetworkError::RecordConflict)
            })
            .collect()
    }

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
        observed_revisions: &[String],
        mutation_id: &str,
    ) -> Result<LocalRecordUpdate, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let requested_revisions =
            requested_record_revisions(observed_revision, observed_revisions)?;
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
                || previous.observed_revisions() != requested_revisions
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

        self.validate_current_record_revisions(
            address.path(),
            &requested_revisions,
            RequiredSingleRecordState::Present,
        )?;

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
                observed_revisions: &requested_revisions,
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
        observed_revisions: &[String],
        mutation_id: &str,
    ) -> Result<LocalRecordDelete, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let requested_revisions =
            requested_record_revisions(observed_revision, observed_revisions)?;
        if let Some(previous) = self
            .local_record_mutations
            .get(&identity)
            .and_then(|mutations| mutations.get(mutation_id))
        {
            if previous.operation() != PersistedRecordMutationOperation::Delete
                || previous.path != address.path()
                || previous.observed_revisions() != requested_revisions
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

        self.validate_current_record_revisions(
            address.path(),
            &requested_revisions,
            RequiredSingleRecordState::Present,
        )?;

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
                observed_revisions: &requested_revisions,
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
        observed_revisions: &[String],
        mutation_id: &str,
    ) -> Result<LocalRecordRestore, NetworkError> {
        validate_record_mutation_id(mutation_id)?;
        let identity = self.identity.identity_id();
        let address = JoltAddress::new(identity.clone(), path)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let requested_revisions =
            requested_record_revisions(observed_revision, observed_revisions)?;
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
                || previous.observed_revisions() != requested_revisions
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

        self.validate_current_record_revisions(
            address.path(),
            &requested_revisions,
            RequiredSingleRecordState::Deleted,
        )?;

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
                observed_revisions: &requested_revisions,
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
        if self
            .blocked_local_device_writer_identities
            .contains(&identity)
        {
            return Err(NetworkError::Protocol(format!(
                "local device-writer history diverged for {identity}; refusing to append"
            )));
        }
        let created_at = unix_now();
        let observed_heads = match record_mutation {
            Some(ref mutation) => {
                self.observed_record_head_hashes(&identity, mutation.observed_revisions)?
            }
            None => self.current_singleton_heads(&identity, &operation),
        };
        let local_device_id = self.local_device_id();
        let entry = match self
            .local_device_writer_logs
            .get(&identity)
            .and_then(|entries| entries.last())
        {
            Some(previous) => previous
                .append_observing(operation, observed_heads, created_at, |bytes| {
                    self.local_device_identity.sign(bytes)
                })
                .map_err(|e| NetworkError::Protocol(e.to_string()))?,
            None => DeviceWriterLogEntry::genesis_observing(
                identity.clone(),
                local_device_id.clone(),
                operation,
                observed_heads,
                created_at,
                |bytes| self.local_device_identity.sign(bytes),
            )
            .map_err(|e| NetworkError::Protocol(e.to_string()))?,
        };

        let authority_records =
            self.authority_records_for_local_device(&identity, &local_device_id, created_at, true)?;
        let device_sequence = entry.body.device_sequence;
        let revision = entry.entry_hash().to_hex();
        let mut device_log = self
            .local_device_writer_logs
            .get(&identity)
            .cloned()
            .unwrap_or_default();
        device_log.push(entry);
        let other_device_logs = self.other_device_logs_for_persistence(&identity, &local_device_id);
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
                    observed_revision: record_mutation
                        .observed_revisions
                        .last()
                        .cloned()
                        .ok_or_else(|| {
                            NetworkError::Protocol(
                                "record mutation must observe at least one revision".to_string(),
                            )
                        })?,
                    observed_revisions: if record_mutation.observed_revisions.len() > 1 {
                        record_mutation.observed_revisions.to_vec()
                    } else {
                        Vec::new()
                    },
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
                    other_device_logs,
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

        self.refresh_local_device_writer_state_from_connected_peer();

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

    fn authority_records_for_local_device(
        &self,
        identity: &IdentityId,
        local_device_id: &str,
        created_at: u64,
        require_active: bool,
    ) -> Result<Vec<DeviceAuthorizationRecord>, NetworkError> {
        let mut records = self
            .local_device_authority_records
            .get(identity)
            .cloned()
            .unwrap_or_default();
        if !records.is_empty() {
            let authority = verify_identity_authority_chain(identity, &records)
                .map_err(|error| NetworkError::Protocol(error.to_string()))?;
            if let Some(device) = authority.devices.get(local_device_id) {
                if require_active && device.status != AuthorizedDeviceStatus::Active {
                    return Err(NetworkError::LocalDeviceRevoked {
                        device_id: local_device_id.to_string(),
                    });
                }
                if device.signing_public_key != self.local_device_identity.public_key_bytes() {
                    return Err(NetworkError::LocalDeviceSigningKeyMismatch {
                        device_id: local_device_id.to_string(),
                    });
                }
                return Ok(records);
            }
        }

        let operation = DeviceAuthorizationOperation::authorize_device(
            local_device_id,
            self.local_device_identity.public_key_bytes(),
            vec!["identity:write".to_string()],
            Some("Local device".to_string()),
            created_at,
        );
        let record = match records.last() {
            Some(previous) => {
                previous.append(operation, created_at, |bytes| self.identity.sign(bytes))
            }
            None => DeviceAuthorizationRecord::genesis(
                self.identity.public_key_bytes(),
                identity.clone(),
                operation,
                created_at,
                |bytes| self.identity.sign(bytes),
            ),
        }
        .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        records.push(record);
        Ok(records)
    }

    pub(super) fn ensure_local_device_authority(&mut self) -> Result<(), NetworkError> {
        let identity = self.identity.identity_id();
        let local_device_id = self.local_device_id();
        let authority_records = self.authority_records_for_local_device(
            &identity,
            &local_device_id,
            unix_now(),
            false,
        )?;
        self.persist_device_writer_state(&identity, &authority_records)?;
        self.store_verified_device_writer_logs(
            identity.clone(),
            authority_records.clone(),
            Vec::new(),
        )?;
        self.local_device_authority_records
            .insert(identity, authority_records);
        Ok(())
    }

    pub(super) fn local_device_authority_records(&self) -> Vec<DeviceAuthorizationRecord> {
        self.local_device_authority_records
            .get(&self.identity.identity_id())
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn append_local_device_authority_operation(
        &mut self,
        operation: DeviceAuthorizationOperation,
    ) -> Result<Vec<DeviceAuthorizationRecord>, NetworkError> {
        let identity = self.identity.identity_id();
        let mut authority_records = self.local_device_authority_records();
        let previous = authority_records.last().ok_or_else(|| {
            NetworkError::Protocol("local device authority is not initialized".to_string())
        })?;
        let created_at = unix_now();
        let record = previous
            .append(operation, created_at, |bytes| self.identity.sign(bytes))
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        authority_records.push(record);
        verify_identity_authority_chain(&identity, &authority_records)
            .map_err(|error| NetworkError::InvalidInput(error.to_string()))?;
        let authority_bytes = serde_json::to_vec(&authority_records)
            .map_err(|error| NetworkError::Protocol(error.to_string()))?;
        self.publish_bytes_at_path(&authority_bytes, IDENTITY_AUTHORITY_PATH)?;
        self.persist_device_writer_state(&identity, &authority_records)?;
        self.store_verified_device_writer_logs(
            identity.clone(),
            authority_records.clone(),
            Vec::new(),
        )?;
        self.local_device_authority_records
            .insert(identity, authority_records.clone());
        Ok(authority_records)
    }

    fn persist_device_writer_state(
        &self,
        identity: &IdentityId,
        authority_records: &[DeviceAuthorizationRecord],
    ) -> Result<(), NetworkError> {
        let local_device_id = self.local_device_id();
        let device_log = self
            .local_device_writer_logs
            .get(identity)
            .cloned()
            .unwrap_or_default();
        let other_device_logs = self.other_device_logs_for_persistence(identity, &local_device_id);
        self.store
            .save_device_writer_log(
                identity,
                &PersistedDeviceWriterLog {
                    authority_records: authority_records.to_vec(),
                    device_log,
                    other_device_logs,
                    record_mutations: self
                        .local_record_mutations
                        .get(identity)
                        .cloned()
                        .unwrap_or_default(),
                },
            )
            .map_err(|error| {
                NetworkError::Protocol(format!(
                    "failed to persist device-writer state for {identity}: {error}"
                ))
            })
    }

    pub(super) fn persist_synced_local_device_writer_state(
        &mut self,
        identity: &IdentityId,
    ) -> Result<(), NetworkError> {
        if identity != &self.identity.identity_id() {
            return Ok(());
        }
        let (authority_records, synced_local_log) = self
            .device_writer_states
            .get(identity)
            .map(|state| {
                (
                    state.authority_records.clone(),
                    state.device_logs.get(&self.local_device_id()).cloned(),
                )
            })
            .ok_or_else(|| {
                NetworkError::Protocol(format!(
                    "cannot persist missing device-writer state for {identity}"
                ))
            })?;
        if let Some(synced_local_log) = synced_local_log {
            let local_log = self
                .local_device_writer_logs
                .get(identity)
                .cloned()
                .unwrap_or_default();
            if super::resolution::device_log_is_prefix(&local_log, &synced_local_log) {
                self.local_device_writer_logs
                    .insert(identity.clone(), synced_local_log);
                self.blocked_local_device_writer_identities.remove(identity);
            } else if !super::resolution::device_log_is_prefix(&synced_local_log, &local_log) {
                self.blocked_local_device_writer_identities
                    .insert(identity.clone());
                return Err(NetworkError::Protocol(format!(
                    "local device-writer history diverged for {identity}"
                )));
            }
        }
        self.persist_device_writer_state(identity, &authority_records)?;
        self.local_device_authority_records
            .insert(identity.clone(), authority_records);
        Ok(())
    }

    fn other_device_logs_for_persistence(
        &self,
        identity: &IdentityId,
        local_device_id: &str,
    ) -> Vec<Vec<DeviceWriterLogEntry>> {
        let mut logs: Vec<Vec<DeviceWriterLogEntry>> = self
            .device_writer_states
            .get(identity)
            .map(|state| {
                state
                    .device_logs
                    .iter()
                    .filter(|(device_id, _)| *device_id != &local_device_id)
                    .map(|(_, log)| log.clone())
                    .collect()
            })
            .unwrap_or_default();
        logs.sort_by(|left, right| {
            let left_id = left
                .first()
                .map(|entry| entry.body.device_id.as_str())
                .unwrap_or("");
            let right_id = right
                .first()
                .map(|entry| entry.body.device_id.as_str())
                .unwrap_or("");
            left_id.cmp(right_id)
        });
        logs
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
        let device_logs = record.device_logs();
        self.store_verified_device_writer_logs(
            identity_id.clone(),
            record.authority_records.clone(),
            device_logs.clone(),
        )
        .map_err(|e| {
            NetworkError::Protocol(format!(
                "invalid persisted device-writer log for {identity_id}: {e}"
            ))
        })?;

        self.local_device_authority_records
            .insert(identity_id.clone(), record.authority_records);
        let local_device_id = self.local_device_id();
        if let Some(local_device_log) = device_logs.into_iter().find(|log| {
            log.first()
                .is_some_and(|entry| entry.body.device_id == local_device_id)
        }) {
            self.local_device_writer_logs
                .insert(identity_id.clone(), local_device_log);
        }
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
