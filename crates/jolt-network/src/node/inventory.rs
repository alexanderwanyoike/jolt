use std::collections::{HashMap, HashSet};

use jolt_core::{
    ContentId, DeviceWriterLogEntry, DeviceWriterLogEntryHash, DeviceWriterOperation,
    DeviceWriterPathMode, DeviceWriterPathState, JoltAddress, MergedDeviceWriterEntry,
    UpdateAction,
};
use jolt_store::HomeRelayPinRecord;

use crate::command::{
    LocalRecordHead, LocalRecordInfo, LocalRecordState, PublishedContentInfo, PublishedRelayInfo,
};
use crate::config::HomeRelayConfig;
use crate::error::NetworkError;

use super::{unix_now, CachedDeviceWriterState, NetworkNode};

fn local_record_head(entry: &MergedDeviceWriterEntry, path: &str) -> LocalRecordHead {
    let revision = entry.entry_hash.to_hex();
    match &entry.state {
        DeviceWriterPathState::Present { content_id } => {
            LocalRecordHead::Present(LocalRecordInfo {
                path: path.to_string(),
                content_id: content_id.to_string(),
                revision,
            })
        }
        DeviceWriterPathState::Tombstone => LocalRecordHead::Deleted { revision },
    }
}

fn entry_parent_hashes(entry: &DeviceWriterLogEntry) -> Vec<DeviceWriterLogEntryHash> {
    entry
        .body
        .previous_entry_hash
        .iter()
        .chain(entry.body.observed_heads.iter())
        .cloned()
        .collect()
}

fn ancestor_hashes(
    head: &DeviceWriterLogEntryHash,
    entries: &HashMap<DeviceWriterLogEntryHash, &DeviceWriterLogEntry>,
) -> HashSet<DeviceWriterLogEntryHash> {
    let mut ancestors = HashSet::new();
    let mut pending = entries
        .get(head)
        .map(|entry| entry_parent_hashes(entry))
        .unwrap_or_default();
    while let Some(revision) = pending.pop() {
        if !ancestors.insert(revision.clone()) {
            continue;
        }
        if let Some(entry) = entries.get(&revision) {
            pending.extend(entry_parent_hashes(entry));
        }
    }
    ancestors
}

fn entry_is_singleton_path(entry: &DeviceWriterLogEntry, path: &str) -> bool {
    match &entry.body.operation {
        DeviceWriterOperation::SetPath {
            path: entry_path,
            mode: DeviceWriterPathMode::Singleton,
            ..
        }
        | DeviceWriterOperation::TombstonePath { path: entry_path } => entry_path == path,
        DeviceWriterOperation::SetPath { .. } => false,
    }
}

fn local_record_head_from_entry(
    entry: &DeviceWriterLogEntry,
    path: &str,
) -> Option<LocalRecordHead> {
    let revision = entry.entry_hash().to_hex();
    match &entry.body.operation {
        DeviceWriterOperation::SetPath {
            path: entry_path,
            content_id,
            mode: DeviceWriterPathMode::Singleton,
        } if entry_path == path => Some(LocalRecordHead::Present(LocalRecordInfo {
            path: path.to_string(),
            content_id: content_id.to_string(),
            revision,
        })),
        DeviceWriterOperation::TombstonePath { path: entry_path } if entry_path == path => {
            Some(LocalRecordHead::Deleted { revision })
        }
        _ => None,
    }
}

fn local_record_common_base(
    state: &CachedDeviceWriterState,
    path: &str,
    heads: &[MergedDeviceWriterEntry],
) -> Option<LocalRecordHead> {
    if heads.len() < 2 {
        return None;
    }
    let entries: HashMap<_, _> = state
        .device_logs
        .values()
        .flatten()
        .map(|entry| (entry.entry_hash(), entry))
        .collect();
    let mut common = ancestor_hashes(&heads[0].entry_hash, &entries);
    for head in &heads[1..] {
        let ancestors = ancestor_hashes(&head.entry_hash, &entries);
        common.retain(|revision| ancestors.contains(revision));
    }
    let candidates: Vec<_> = common
        .into_iter()
        .filter(|revision| {
            entries
                .get(revision)
                .is_some_and(|entry| entry_is_singleton_path(entry, path))
        })
        .collect();
    let latest: Vec<_> = candidates
        .iter()
        .filter(|candidate| {
            !candidates.iter().any(|other| {
                other != *candidate && ancestor_hashes(other, &entries).contains(*candidate)
            })
        })
        .collect();
    if latest.len() != 1 {
        return None;
    }
    entries
        .get(latest[0])
        .and_then(|entry| local_record_head_from_entry(entry, path))
}

impl NetworkNode {
    pub(super) fn inspect_local_record(&self, path: &str) -> LocalRecordState {
        let identity = self.identity.identity_id();
        let Some(state) = self.device_writer_states.get(&identity) else {
            return LocalRecordState::Missing {
                path: path.to_string(),
            };
        };
        let Some(entry) = state.merged.singleton_paths.get(path) else {
            return LocalRecordState::Missing {
                path: path.to_string(),
            };
        };
        if let Some(conflicts) = state.merged.singleton_conflicts.get(path) {
            let mut heads = conflicts.clone();
            heads.push(entry.clone());
            let base = local_record_common_base(state, path, &heads);
            return LocalRecordState::Conflicted {
                path: path.to_string(),
                alternatives: heads
                    .iter()
                    .map(|head| local_record_head(head, path))
                    .collect(),
                base,
            };
        }
        let revision = entry.entry_hash.to_hex();
        match &entry.state {
            DeviceWriterPathState::Present { content_id } => {
                LocalRecordState::Present(LocalRecordInfo {
                    path: path.to_string(),
                    content_id: content_id.to_string(),
                    revision,
                })
            }
            DeviceWriterPathState::Tombstone => LocalRecordState::Deleted {
                path: path.to_string(),
                revision,
            },
        }
    }

    pub(super) fn published_content_inventory(&self) -> Vec<PublishedContentInfo> {
        let identity = self.identity.identity_id();
        let current_paths = self.current_local_paths();

        let mut content_paths: HashMap<String, (String, u64)> = HashMap::new();
        for (path, (content_id, sequence)) in &current_paths {
            content_paths.insert(content_id.to_string(), (path.clone(), *sequence));
        }

        let pin_records = self.store.load_home_relay_pin_records().unwrap_or_default();

        self.store
            .list_published_content()
            .into_iter()
            .filter(|entry| entry.content_type != "application/jolt-update-log+json")
            .map(|entry| {
                let path = content_paths.get(&entry.content_id).cloned();
                let (path, local_sequence) = match path {
                    Some((path, sequence)) => (Some(path), Some(sequence)),
                    None => (None, None),
                };
                let address = path
                    .as_deref()
                    .and_then(|path| JoltAddress::new(identity.clone(), path).ok())
                    .map(|address| address.to_string());
                let pin_record =
                    Self::matching_pin_record(&pin_records, path.as_deref(), &entry.content_id);
                let pin_state = match (&pin_record, local_sequence) {
                    (Some(record), Some(sequence))
                        if record.content_id == entry.content_id
                            && record.latest_sequence >= sequence =>
                    {
                        "relay_backed"
                    }
                    (Some(_), Some(_)) => "needs_repin",
                    (Some(record), None) if record.content_id == entry.content_id => "relay_backed",
                    _ => "local_only",
                }
                .to_string();

                PublishedContentInfo {
                    content_id: entry.content_id,
                    size: entry.size,
                    path,
                    address,
                    local_sequence,
                    pin_state,
                    relay: pin_record.map(|record| PublishedRelayInfo {
                        peer_id: record.relay_peer_id.clone(),
                        multiaddr: record.relay_multiaddr.clone(),
                        api_url: record.relay_api_url.clone(),
                    }),
                    pinned_content_id: pin_record.map(|record| record.content_id.clone()),
                    pinned_sequence: pin_record.map(|record| record.latest_sequence),
                }
            })
            .collect()
    }

    pub(super) fn current_local_paths(&self) -> HashMap<String, (ContentId, u64)> {
        let identity = self.identity.identity_id();
        let mut current_paths: HashMap<String, (ContentId, u64)> = HashMap::new();
        if let Some(entries) = self.update_logs.get(&identity) {
            for entry in entries {
                match &entry.body.action {
                    UpdateAction::SetPath { path, content_id } => {
                        current_paths
                            .insert(path.clone(), (content_id.clone(), entry.body.sequence));
                    }
                    UpdateAction::RemovePath { path } => {
                        current_paths.remove(path);
                    }
                    _ => {}
                }
            }
        }
        current_paths
    }

    fn matching_pin_record<'a>(
        records: &'a [HomeRelayPinRecord],
        path: Option<&str>,
        content_id: &str,
    ) -> Option<&'a HomeRelayPinRecord> {
        records
            .iter()
            .filter(|record| match (path, record.path.as_deref()) {
                (Some(path), Some(record_path)) => path == record_path,
                (None, _) => record.content_id == content_id,
                _ => false,
            })
            .max_by_key(|record| (record.latest_sequence, record.pinned_at))
    }

    pub(super) fn record_home_relay_pin(
        &self,
        content_id: &str,
        requested_path: Option<String>,
        relay: HomeRelayConfig,
        latest_sequence: u64,
    ) -> Result<(), NetworkError> {
        let identity = self.identity.identity_id();
        let current_paths = self.current_local_paths();
        let paths = if let Some(path) = requested_path {
            match current_paths.get(&path) {
                Some((path_content_id, _)) if path_content_id.to_string() == content_id => {
                    vec![Some(path)]
                }
                Some(_) => {
                    return Err(NetworkError::InvalidInput(format!(
                        "path {path} does not point at content {content_id}"
                    )));
                }
                None => {
                    return Err(NetworkError::InvalidInput(format!(
                        "path {path} is not locally published"
                    )));
                }
            }
        } else {
            let mut paths = current_paths
                .iter()
                .filter_map(|(path, (path_content_id, _))| {
                    if path_content_id.to_string() == content_id {
                        Some(Some(path.clone()))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            if paths.is_empty() {
                paths.push(None);
            }
            paths
        };

        for path in paths {
            let address = path
                .as_deref()
                .and_then(|path| JoltAddress::new(identity.clone(), path).ok())
                .map(|address| address.to_string());
            let record = HomeRelayPinRecord {
                content_id: content_id.to_string(),
                path,
                address,
                relay_peer_id: relay.peer_id.clone(),
                relay_multiaddr: relay.multiaddr.clone(),
                relay_api_url: relay.api_url.clone(),
                latest_sequence,
                pinned_at: unix_now(),
            };
            self.store
                .save_home_relay_pin_record(record)
                .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        }

        Ok(())
    }
}
