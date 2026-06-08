use std::collections::HashMap;

use jolt_core::{ContentId, JoltAddress, UpdateAction};
use jolt_store::HomeRelayPinRecord;

use crate::command::{PublishedContentInfo, PublishedRelayInfo};
use crate::config::HomeRelayConfig;
use crate::error::NetworkError;

use super::{unix_now, NetworkNode};

impl NetworkNode {
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
