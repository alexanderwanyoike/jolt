use serde::{Deserialize, Serialize};

use jolt_core::{
    DeviceAuthorizationRecord, DeviceWriterLogEntry, DeviceWriterLogEntryHash, IdentityHeadHint,
    IdentityId, RelayRecord, RelayRecordCapability, UpdateLogEntry,
};

use crate::command::IngressRecord;

pub const LEGACY_DEVICE_WRITER_OPERATION_VERSION: u16 = 1;
pub const TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION: u16 = 2;
pub const CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION: u16 = 3;
pub const LEGACY_DEVICE_WRITER_SYNC_VERSION: u16 = 1;
pub const DELTA_DEVICE_WRITER_SYNC_VERSION: u16 = 2;
pub const DEVICE_WRITER_DELTA_MAX_ENTRIES: usize = 256;
pub const DEVICE_WRITER_DELTA_MAX_RESPONSE_BYTES: usize = 1024 * 1024;

fn legacy_device_writer_operation_version() -> u16 {
    LEGACY_DEVICE_WRITER_OPERATION_VERSION
}

fn legacy_device_writer_sync_version() -> u16 {
    LEGACY_DEVICE_WRITER_SYNC_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentRequest {
    pub content_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentResponse {
    pub data: Vec<u8>,
    pub signature: Vec<u8>,
    pub publisher_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLogRequest {
    pub identity: IdentityId,
    pub since: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLogResponse {
    pub entries: Vec<UpdateLogEntry>,
}

/// Ask a provider for an identity's device-authority records and the per-device
/// writer logs it holds. This mirrors `UpdateLogRequest` but carries the
/// multi-writer device-log state instead of the legacy single-writer log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceWriterSyncRequest {
    pub identity: IdentityId,
    #[serde(default = "legacy_device_writer_operation_version")]
    pub max_operation_version: u16,
    #[serde(default = "legacy_device_writer_sync_version")]
    pub max_sync_version: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cursors: Vec<DeviceWriterCursor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_records: Vec<DeviceAuthorizationRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub device_logs: Vec<Vec<DeviceWriterLogEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceWriterCursor {
    pub device_id: String,
    pub device_sequence: u64,
    pub entry_hash: DeviceWriterLogEntryHash,
}

impl DeviceWriterCursor {
    pub fn from_entry(entry: &DeviceWriterLogEntry) -> Self {
        Self {
            device_id: entry.body.device_id.clone(),
            device_sequence: entry.body.device_sequence,
            entry_hash: entry.entry_hash(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceWriterSyncContinuation {
    pub cursors: Vec<DeviceWriterCursor>,
}

impl DeviceWriterSyncRequest {
    pub fn new(identity: IdentityId) -> Self {
        Self {
            identity,
            max_operation_version: CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
            max_sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
            cursors: Vec::new(),
            authority_records: Vec::new(),
            device_logs: Vec::new(),
        }
    }

    pub fn with_cursors(mut self, cursors: Vec<DeviceWriterCursor>) -> Self {
        self.cursors = cursors;
        self
    }

    pub fn offering(
        identity: IdentityId,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Self {
        Self {
            identity,
            max_operation_version: CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
            max_sync_version: LEGACY_DEVICE_WRITER_SYNC_VERSION,
            cursors: Vec::new(),
            authority_records,
            device_logs,
        }
    }
}

/// A provider's view of an identity's device-writer state: the verified
/// device-authority chain plus every per-device writer log it can serve. The
/// requester re-verifies and re-merges these locally, so an unverified or
/// hostile response cannot poison the cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceWriterSyncResponse {
    #[serde(default = "legacy_device_writer_operation_version")]
    pub required_operation_version: u16,
    #[serde(default = "legacy_device_writer_sync_version")]
    pub sync_version: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub heads: Vec<DeviceWriterCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<DeviceWriterSyncContinuation>,
    pub authority_records: Vec<DeviceAuthorizationRecord>,
    pub device_logs: Vec<Vec<DeviceWriterLogEntry>>,
}

impl DeviceWriterSyncResponse {
    pub fn unsupported_operation_version(required_operation_version: u16) -> Self {
        Self {
            required_operation_version,
            sync_version: LEGACY_DEVICE_WRITER_SYNC_VERSION,
            heads: Vec::new(),
            continuation: None,
            authority_records: Vec::new(),
            device_logs: Vec::new(),
        }
    }

    pub fn for_request(
        max_operation_version: u16,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Self {
        let required_operation_version = device_logs
            .iter()
            .flatten()
            .map(|entry| {
                if !entry.body.observed_heads.is_empty() {
                    return CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION;
                }

                match entry.body.operation {
                    jolt_core::DeviceWriterOperation::SetPath { .. } => {
                        LEGACY_DEVICE_WRITER_OPERATION_VERSION
                    }
                    jolt_core::DeviceWriterOperation::TombstonePath { .. } => {
                        TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION
                    }
                }
            })
            .max()
            .unwrap_or(LEGACY_DEVICE_WRITER_OPERATION_VERSION);

        if max_operation_version < required_operation_version {
            return Self::unsupported_operation_version(required_operation_version);
        }

        Self {
            required_operation_version,
            sync_version: LEGACY_DEVICE_WRITER_SYNC_VERSION,
            heads: Vec::new(),
            continuation: None,
            authority_records,
            device_logs,
        }
    }

    pub fn for_sync_request(
        request: &DeviceWriterSyncRequest,
        authority_records: Vec<DeviceAuthorizationRecord>,
        device_logs: Vec<Vec<DeviceWriterLogEntry>>,
    ) -> Self {
        Self::for_sync_request_with_limits(
            request,
            authority_records,
            device_logs,
            DEVICE_WRITER_DELTA_MAX_ENTRIES,
            DEVICE_WRITER_DELTA_MAX_RESPONSE_BYTES,
        )
    }

    fn for_sync_request_with_limits(
        request: &DeviceWriterSyncRequest,
        authority_records: Vec<DeviceAuthorizationRecord>,
        mut device_logs: Vec<Vec<DeviceWriterLogEntry>>,
        max_entries: usize,
        max_bytes: usize,
    ) -> Self {
        let full = Self::for_request(
            request.max_operation_version,
            authority_records,
            device_logs.clone(),
        );
        if request.max_sync_version < DELTA_DEVICE_WRITER_SYNC_VERSION
            || full.required_operation_version > request.max_operation_version
        {
            return full;
        }

        device_logs.sort_by(|left, right| {
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
        let mut cursors = std::collections::HashMap::new();
        for cursor in &request.cursors {
            if cursors
                .insert(cursor.device_id.clone(), cursor.clone())
                .is_some()
            {
                return full;
            }
        }

        let heads: Vec<_> = device_logs
            .iter()
            .filter_map(|log| log.last().map(DeviceWriterCursor::from_entry))
            .collect();
        let mut starts = Vec::with_capacity(device_logs.len());
        for log in &device_logs {
            let Some(head) = log.last() else {
                starts.push(0);
                continue;
            };
            let start = match cursors.get(&head.body.device_id) {
                None => 0,
                Some(cursor) => {
                    let Some(known) = log.get(cursor.device_sequence as usize) else {
                        return full;
                    };
                    if known.body.device_sequence != cursor.device_sequence
                        || known.entry_hash() != cursor.entry_hash
                    {
                        return full;
                    }
                    cursor.device_sequence as usize + 1
                }
            };
            starts.push(start);
        }
        if cursors
            .keys()
            .any(|device_id| !heads.iter().any(|head| &head.device_id == device_id))
        {
            return full;
        }

        let mut delta_logs = Vec::new();
        let mut transferred_entries = 0;
        let mut transferred_bytes: usize = 0;
        let mut truncated = false;
        for (log, start) in device_logs.iter().zip(starts) {
            let mut delta = Vec::new();
            for entry in &log[start..] {
                let mut encoded = Vec::new();
                ciborium::into_writer(entry, &mut encoded)
                    .expect("device-writer entries serialize to CBOR");
                if transferred_entries >= max_entries
                    || transferred_bytes.saturating_add(encoded.len()) > max_bytes
                {
                    truncated = true;
                    break;
                }
                transferred_entries += 1;
                transferred_bytes += encoded.len();
                delta.push(entry.clone());
            }
            if !delta.is_empty() {
                delta_logs.push(delta);
            }
            if truncated {
                break;
            }
        }

        if truncated && transferred_entries == 0 {
            return full;
        }
        let build_response = |device_logs: &[Vec<DeviceWriterLogEntry>], truncated: bool| {
            let continuation = truncated.then(|| {
                let mut cursors: std::collections::HashMap<_, _> = request
                    .cursors
                    .iter()
                    .cloned()
                    .map(|cursor| (cursor.device_id.clone(), cursor))
                    .collect();
                for entry in device_logs.iter().flatten() {
                    cursors.insert(
                        entry.body.device_id.clone(),
                        DeviceWriterCursor::from_entry(entry),
                    );
                }
                let mut cursors: Vec<_> = cursors.into_values().collect();
                cursors.sort_by(|left, right| left.device_id.cmp(&right.device_id));
                DeviceWriterSyncContinuation { cursors }
            });
            Self {
                required_operation_version: full.required_operation_version,
                sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION,
                heads: heads.clone(),
                continuation,
                authority_records: full.authority_records.clone(),
                device_logs: device_logs.to_vec(),
            }
        };

        loop {
            let response = build_response(&delta_logs, truncated);
            let mut encoded = Vec::new();
            ciborium::into_writer(&response, &mut encoded)
                .expect("device-writer sync responses serialize to CBOR");
            if encoded.len() <= max_bytes {
                return response;
            }
            if transferred_entries == 0 {
                return full;
            }

            let last_log = delta_logs
                .last_mut()
                .expect("a transferred entry has a delta log");
            last_log.pop();
            if last_log.is_empty() {
                delta_logs.pop();
            }
            transferred_entries -= 1;
            truncated = true;
        }
    }

    pub fn ensure_supported(
        &self,
        supported_operation_version: u16,
        supported_sync_version: u16,
    ) -> Result<(), crate::error::NetworkError> {
        if self.required_operation_version > supported_operation_version {
            Err(
                crate::error::NetworkError::UnsupportedDeviceWriterOperationVersion {
                    supported: supported_operation_version,
                    required: self.required_operation_version,
                },
            )
        } else if self.sync_version > supported_sync_version {
            Err(
                crate::error::NetworkError::UnsupportedDeviceWriterSyncVersion {
                    supported: supported_sync_version,
                    required: self.sync_version,
                },
            )
        } else {
            Ok(())
        }
    }
}

impl Default for DeviceWriterSyncResponse {
    fn default() -> Self {
        Self {
            required_operation_version: LEGACY_DEVICE_WRITER_OPERATION_VERSION,
            sync_version: LEGACY_DEVICE_WRITER_SYNC_VERSION,
            heads: Vec::new(),
            continuation: None,
            authority_records: Vec::new(),
            device_logs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelayExchangeRequest {
    GetRelays {
        limit: u16,
        capabilities: Vec<RelayRecordCapability>,
    },
    AnnounceRelays {
        records: Vec<RelayRecord>,
    },
    GetIdentityHeads {
        limit: u16,
    },
    AnnounceIdentityHeads {
        hints: Vec<IdentityHeadHint>,
    },
    FindIdentityProviders {
        query_id: String,
        identity: IdentityId,
        limit: u16,
        ttl: u8,
        deadline_unix_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelayExchangeResponse {
    Relays {
        records: Vec<RelayRecord>,
    },
    Announced {
        accepted: u16,
        rejected: u16,
    },
    IdentityHeads {
        hints: Vec<IdentityHeadHint>,
    },
    IdentityProviders {
        query_id: String,
        identity: IdentityId,
        providers: Vec<IdentityProviderCandidate>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdentityProviderCandidate {
    pub peer_id: String,
    pub addrs: Vec<String>,
}

/// Deliver a recipient-controlled ingress envelope to the recipient's own
/// daemon over p2p. The recipient re-validates the envelope (signature and
/// addressing) before queueing it, exactly as its trusted HTTP submit route
/// does, so a hostile sender gains nothing beyond what HTTP submission allows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressSubmitRequest {
    pub receiver_id: String,
    pub encrypted_object: Vec<u8>,
    pub expires_at: Option<u64>,
}

/// The recipient's verdict. `Accepted` carries the queued record (metadata
/// only; the encrypted bytes never travel back). The sender must verify the
/// record's `recipient_identity` is the identity it meant to reach - a
/// delivery that lands anywhere else is a failure, never a success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IngressSubmitResponse {
    Accepted { record: IngressRecord },
    Rejected { error: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolt_core::{
        ContentId, IdentityHeadHint, IdentityId, UpdateAction, UpdateLogEntry, UpdateLogEntryHash,
    };
    use jolt_identity::NodeIdentity;

    #[test]
    fn content_request_cbor_round_trip() {
        let request = ContentRequest {
            content_id: "bafk_test_id_123".to_string(),
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: ContentRequest = ciborium::from_reader(&buf[..]).unwrap();
        assert_eq!(request.content_id, decoded.content_id);
    }

    #[test]
    fn content_response_cbor_round_trip() {
        let response = ContentResponse {
            data: vec![1, 2, 3, 4, 5],
            signature: vec![10, 20, 30],
            publisher_key: vec![40, 50, 60],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: ContentResponse = ciborium::from_reader(&buf[..]).unwrap();
        assert_eq!(response.data, decoded.data);
        assert_eq!(response.signature, decoded.signature);
        assert_eq!(response.publisher_key, decoded.publisher_key);
    }

    #[test]
    fn update_log_request_cbor_round_trip() {
        let identity = IdentityId::from_public_key([9; 32]);
        let request = UpdateLogRequest {
            identity: identity.clone(),
            since: Some(7),
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: UpdateLogRequest = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded.identity, identity);
        assert_eq!(decoded.since, Some(7));
    }

    #[test]
    fn update_log_response_cbor_round_trip() {
        let identity = NodeIdentity::generate();
        let entry = UpdateLogEntry::genesis(
            identity.public_key_bytes(),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: ContentId::from_bytes(b"profile"),
            },
            |bytes| identity.sign(bytes),
        )
        .unwrap();
        let response = UpdateLogResponse {
            entries: vec![entry.clone()],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: UpdateLogResponse = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded.entries, vec![entry]);
    }

    #[test]
    fn device_writer_sync_request_cbor_round_trip() {
        let identity = IdentityId::from_public_key([5; 32]);
        let request = DeviceWriterSyncRequest::new(identity.clone());

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: DeviceWriterSyncRequest = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded.identity, identity);
        assert_eq!(decoded.max_sync_version, DELTA_DEVICE_WRITER_SYNC_VERSION);
        assert!(decoded.cursors.is_empty());
        assert!(decoded.authority_records.is_empty());
        assert!(decoded.device_logs.is_empty());
    }

    #[test]
    fn device_writer_sync_request_is_compatible_with_legacy_peers() {
        #[derive(Serialize, Deserialize)]
        struct LegacyDeviceWriterSyncRequest {
            identity: IdentityId,
        }

        let identity = IdentityId::from_public_key([6; 32]);
        let current = DeviceWriterSyncRequest::new(identity.clone());
        assert_eq!(
            current.max_operation_version,
            CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION
        );

        let mut current_bytes = Vec::new();
        ciborium::into_writer(&current, &mut current_bytes).unwrap();
        let decoded_by_legacy: LegacyDeviceWriterSyncRequest =
            ciborium::from_reader(&current_bytes[..]).unwrap();
        assert_eq!(decoded_by_legacy.identity, identity);

        let mut legacy_bytes = Vec::new();
        ciborium::into_writer(
            &LegacyDeviceWriterSyncRequest {
                identity: identity.clone(),
            },
            &mut legacy_bytes,
        )
        .unwrap();
        let decoded_by_current: DeviceWriterSyncRequest =
            ciborium::from_reader(&legacy_bytes[..]).unwrap();
        assert_eq!(decoded_by_current.identity, identity);
        assert_eq!(decoded_by_current.max_operation_version, 1);
        assert_eq!(
            decoded_by_current.max_sync_version,
            LEGACY_DEVICE_WRITER_SYNC_VERSION
        );
        assert!(decoded_by_current.cursors.is_empty());
        assert!(decoded_by_current.authority_records.is_empty());
        assert!(decoded_by_current.device_logs.is_empty());
    }

    #[test]
    fn device_writer_sync_response_cbor_round_trip() {
        use jolt_core::{
            ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
            DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
        };

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_a",
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap()];
        let device_log = vec![DeviceWriterLogEntry::genesis(
            identity.clone(),
            "dev_a",
            DeviceWriterOperation::set_path(
                "/profile",
                ContentId::from_bytes(b"profile"),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| device.sign(bytes),
        )
        .unwrap()];
        let response = DeviceWriterSyncResponse {
            required_operation_version: LEGACY_DEVICE_WRITER_OPERATION_VERSION,
            sync_version: LEGACY_DEVICE_WRITER_SYNC_VERSION,
            heads: Vec::new(),
            continuation: None,
            authority_records: authority.clone(),
            device_logs: vec![device_log.clone()],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: DeviceWriterSyncResponse = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded.authority_records, authority);
        assert_eq!(decoded.device_logs, vec![device_log]);
        assert_eq!(decoded.sync_version, LEGACY_DEVICE_WRITER_SYNC_VERSION);
    }

    #[test]
    fn device_writer_sync_response_is_compatible_with_legacy_peers() {
        #[derive(Serialize, Deserialize)]
        struct LegacyDeviceWriterSyncResponse {
            authority_records: Vec<DeviceAuthorizationRecord>,
            device_logs: Vec<Vec<DeviceWriterLogEntry>>,
        }

        let unsupported = DeviceWriterSyncResponse::unsupported_operation_version(2);
        let mut current_bytes = Vec::new();
        ciborium::into_writer(&unsupported, &mut current_bytes).unwrap();
        let decoded_by_legacy: LegacyDeviceWriterSyncResponse =
            ciborium::from_reader(&current_bytes[..]).unwrap();
        assert!(decoded_by_legacy.authority_records.is_empty());
        assert!(decoded_by_legacy.device_logs.is_empty());

        let mut legacy_bytes = Vec::new();
        ciborium::into_writer(
            &LegacyDeviceWriterSyncResponse {
                authority_records: Vec::new(),
                device_logs: Vec::new(),
            },
            &mut legacy_bytes,
        )
        .unwrap();
        let decoded_by_current: DeviceWriterSyncResponse =
            ciborium::from_reader(&legacy_bytes[..]).unwrap();
        assert_eq!(decoded_by_current.required_operation_version, 1);
        assert_eq!(
            decoded_by_current.sync_version,
            LEGACY_DEVICE_WRITER_SYNC_VERSION
        );
        assert!(decoded_by_current.heads.is_empty());
        assert!(decoded_by_current.continuation.is_none());
    }

    #[test]
    fn device_writer_sync_refuses_tombstone_history_for_legacy_requester() {
        use jolt_core::{
            DeviceAuthorizationOperation, DeviceAuthorizationRecord, DeviceWriterLogEntry,
            DeviceWriterOperation,
        };

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_a",
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap()];
        let device_logs = vec![vec![DeviceWriterLogEntry::genesis(
            identity,
            "dev_a",
            DeviceWriterOperation::tombstone_path("/posts/post-1"),
            100,
            |bytes| device.sign(bytes),
        )
        .unwrap()]];

        let response = DeviceWriterSyncResponse::for_request(
            LEGACY_DEVICE_WRITER_OPERATION_VERSION,
            authority,
            device_logs,
        );

        assert_eq!(
            response.required_operation_version,
            TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION
        );
        assert!(response.authority_records.is_empty());
        assert!(response.device_logs.is_empty());
    }

    #[test]
    fn device_writer_sync_refuses_causal_history_for_v2_requester() {
        use jolt_core::{
            ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
            DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
        };

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_a",
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap()];
        let first = DeviceWriterLogEntry::genesis(
            identity,
            "dev_a",
            DeviceWriterOperation::set_path(
                "/posts/post-1",
                ContentId::from_bytes(b"first"),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| device.sign(bytes),
        )
        .unwrap();
        let second = first
            .append_observing(
                DeviceWriterOperation::set_path(
                    "/posts/post-1",
                    ContentId::from_bytes(b"resolved"),
                    DeviceWriterPathMode::Singleton,
                ),
                vec![first.entry_hash()],
                101,
                |bytes| device.sign(bytes),
            )
            .unwrap();

        let device_logs = vec![vec![first, second]];
        let response = DeviceWriterSyncResponse::for_request(
            TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION,
            authority.clone(),
            device_logs.clone(),
        );

        assert_eq!(
            response.required_operation_version,
            CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION
        );
        assert!(response.authority_records.is_empty());
        assert!(response.device_logs.is_empty());

        let response = DeviceWriterSyncResponse::for_request(
            CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
            authority.clone(),
            device_logs.clone(),
        );

        assert_eq!(
            response.required_operation_version,
            CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION
        );
        assert_eq!(response.authority_records, authority);
        assert_eq!(response.device_logs, device_logs);
    }

    #[test]
    fn device_writer_sync_serves_complete_restored_history_to_current_requester() {
        use jolt_core::{
            ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
            DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
        };

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_a",
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                Some("Phone".to_string()),
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap()];
        let present = DeviceWriterLogEntry::genesis(
            identity,
            "dev_a",
            DeviceWriterOperation::set_path(
                "/posts/post-1",
                ContentId::from_bytes(b"before delete"),
                DeviceWriterPathMode::Singleton,
            ),
            100,
            |bytes| device.sign(bytes),
        )
        .unwrap();
        let tombstone = present
            .append(
                DeviceWriterOperation::tombstone_path("/posts/post-1"),
                101,
                |bytes| device.sign(bytes),
            )
            .unwrap();
        let restored = tombstone
            .append(
                DeviceWriterOperation::set_path(
                    "/posts/post-1",
                    ContentId::from_bytes(b"after restore"),
                    DeviceWriterPathMode::Singleton,
                ),
                102,
                |bytes| device.sign(bytes),
            )
            .unwrap();
        let device_logs = vec![vec![present, tombstone, restored]];

        let response = DeviceWriterSyncResponse::for_request(
            TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION,
            authority.clone(),
            device_logs.clone(),
        );

        assert_eq!(
            response.required_operation_version,
            TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION
        );
        assert_eq!(response.authority_records, authority);
        assert_eq!(response.device_logs, device_logs);
    }

    #[test]
    fn device_writer_sync_response_reports_unsupported_future_history() {
        let response = DeviceWriterSyncResponse::unsupported_operation_version(4);

        let err = response
            .ensure_supported(
                CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
                DELTA_DEVICE_WRITER_SYNC_VERSION,
            )
            .unwrap_err();

        assert!(matches!(
            err,
            crate::error::NetworkError::UnsupportedDeviceWriterOperationVersion {
                supported: 3,
                required: 4,
            }
        ));
    }

    #[test]
    fn device_writer_sync_response_rejects_future_sync_envelopes() {
        let response = DeviceWriterSyncResponse {
            sync_version: DELTA_DEVICE_WRITER_SYNC_VERSION + 1,
            ..DeviceWriterSyncResponse::default()
        };

        let err = response
            .ensure_supported(
                CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
                DELTA_DEVICE_WRITER_SYNC_VERSION,
            )
            .unwrap_err();

        assert!(matches!(
            err,
            crate::error::NetworkError::UnsupportedDeviceWriterSyncVersion {
                supported: 2,
                required: 3,
            }
        ));
    }

    #[test]
    fn device_writer_delta_omits_verified_history_and_returns_only_new_entries() {
        use jolt_core::{
            ContentId, DeviceAuthorizationOperation, DeviceAuthorizationRecord,
            DeviceWriterLogEntry, DeviceWriterOperation, DeviceWriterPathMode,
        };

        let root = NodeIdentity::generate();
        let device = NodeIdentity::generate();
        let identity = root.identity_id();
        let authority = vec![DeviceAuthorizationRecord::genesis(
            root.public_key_bytes(),
            identity.clone(),
            DeviceAuthorizationOperation::authorize_device(
                "dev_delta",
                device.public_key_bytes(),
                vec!["identity:write".to_string()],
                None,
                100,
            ),
            100,
            |bytes| root.sign(bytes),
        )
        .unwrap()];
        let first = DeviceWriterLogEntry::genesis(
            identity.clone(),
            "dev_delta",
            DeviceWriterOperation::set_path(
                "/posts/first",
                ContentId::from_bytes(b"first"),
                DeviceWriterPathMode::Append,
            ),
            100,
            |bytes| device.sign(bytes),
        )
        .unwrap();
        let second = first
            .append(
                DeviceWriterOperation::set_path(
                    "/posts/second",
                    ContentId::from_bytes(b"second"),
                    DeviceWriterPathMode::Append,
                ),
                101,
                |bytes| device.sign(bytes),
            )
            .unwrap();
        let third = second
            .append(
                DeviceWriterOperation::set_path(
                    "/posts/third",
                    ContentId::from_bytes(b"third"),
                    DeviceWriterPathMode::Append,
                ),
                102,
                |bytes| device.sign(bytes),
            )
            .unwrap();
        let request = DeviceWriterSyncRequest::new(identity)
            .with_cursors(vec![DeviceWriterCursor::from_entry(&first)]);

        let no_change = DeviceWriterSyncResponse::for_sync_request(
            &request,
            authority.clone(),
            vec![vec![first.clone()]],
        );
        assert_eq!(no_change.sync_version, DELTA_DEVICE_WRITER_SYNC_VERSION);
        assert!(no_change.device_logs.is_empty());
        assert_eq!(
            no_change.heads,
            vec![DeviceWriterCursor::from_entry(&first)]
        );
        assert!(no_change.continuation.is_none());

        let one_append = DeviceWriterSyncResponse::for_sync_request(
            &request,
            authority.clone(),
            vec![vec![first.clone(), second.clone()]],
        );
        assert_eq!(one_append.device_logs, vec![vec![second.clone()]]);
        assert_eq!(
            one_append.heads,
            vec![DeviceWriterCursor::from_entry(&second)]
        );
        assert!(one_append.continuation.is_none());

        let mut invalid_cursor = DeviceWriterCursor::from_entry(&first);
        invalid_cursor.entry_hash.0[0] ^= 0xff;
        let invalid_request = DeviceWriterSyncRequest::new(request.identity.clone())
            .with_cursors(vec![invalid_cursor]);
        let recovery = DeviceWriterSyncResponse::for_sync_request(
            &invalid_request,
            Vec::new(),
            vec![vec![first.clone(), second.clone()]],
        );
        assert_eq!(
            recovery.sync_version, LEGACY_DEVICE_WRITER_SYNC_VERSION,
            "an invalid or forked cursor must trigger explicit full-state recovery"
        );
        assert_eq!(
            recovery.device_logs,
            vec![vec![first.clone(), second.clone()]]
        );

        let cold_request = DeviceWriterSyncRequest::new(request.identity.clone());
        let first_page = DeviceWriterSyncResponse::for_sync_request_with_limits(
            &cold_request,
            Vec::new(),
            vec![vec![first.clone(), second.clone(), third.clone()]],
            1,
            usize::MAX,
        );
        assert_eq!(first_page.device_logs, vec![vec![first.clone()]]);
        let continuation = first_page
            .continuation
            .expect("one-entry page must continue");
        assert_eq!(
            continuation.cursors,
            vec![DeviceWriterCursor::from_entry(&first)]
        );

        let second_page_request = DeviceWriterSyncRequest::new(request.identity.clone())
            .with_cursors(continuation.cursors);
        let second_page = DeviceWriterSyncResponse::for_sync_request_with_limits(
            &second_page_request,
            Vec::new(),
            vec![vec![first.clone(), second.clone(), third.clone()]],
            1,
            usize::MAX,
        );
        assert_eq!(second_page.device_logs, vec![vec![second.clone()]]);
        assert_eq!(
            second_page.continuation.unwrap().cursors,
            vec![DeviceWriterCursor::from_entry(&second)]
        );

        let one_entry_page = DeviceWriterSyncResponse::for_sync_request_with_limits(
            &cold_request,
            authority.clone(),
            vec![vec![first.clone(), second.clone(), third.clone()]],
            1,
            usize::MAX,
        );
        let mut one_entry_page_bytes = Vec::new();
        ciborium::into_writer(&one_entry_page, &mut one_entry_page_bytes).unwrap();
        let byte_bounded_page = DeviceWriterSyncResponse::for_sync_request_with_limits(
            &cold_request,
            authority,
            vec![vec![first, second, third]],
            usize::MAX,
            one_entry_page_bytes.len(),
        );
        let mut byte_bounded_page_bytes = Vec::new();
        ciborium::into_writer(&byte_bounded_page, &mut byte_bounded_page_bytes).unwrap();
        assert!(
            byte_bounded_page_bytes.len() <= one_entry_page_bytes.len(),
            "the byte limit applies to the complete encoded delta response"
        );
        assert_eq!(byte_bounded_page.device_logs.iter().flatten().count(), 1);
    }

    #[test]
    fn relay_record_cbor_round_trip() {
        let identity = NodeIdentity::generate();
        let record = RelayRecord::new(
            identity.public_key_bytes(),
            identity.peer_id().to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            vec![
                RelayRecordCapability::Bootstrap,
                RelayRecordCapability::Discovery,
                RelayRecordCapability::Pinning,
            ],
            100,
            200,
            |bytes| identity.sign(bytes),
        )
        .unwrap();

        let mut buf = Vec::new();
        ciborium::into_writer(&record, &mut buf).unwrap();
        let decoded: RelayRecord = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded, record);
        assert_eq!(decoded.verify_at(150), Ok(()));
    }

    #[test]
    fn relay_exchange_get_relays_cbor_round_trip() {
        let request = RelayExchangeRequest::GetRelays {
            limit: 12,
            capabilities: vec![RelayRecordCapability::Bootstrap],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: RelayExchangeRequest = ciborium::from_reader(&buf[..]).unwrap();

        assert!(matches!(
            decoded,
            RelayExchangeRequest::GetRelays {
                limit: 12,
                capabilities
            } if capabilities == vec![RelayRecordCapability::Bootstrap]
        ));
    }

    #[test]
    fn relay_exchange_records_cbor_round_trip() {
        let identity = NodeIdentity::generate();
        let record = RelayRecord::new(
            identity.public_key_bytes(),
            identity.peer_id().to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            vec![RelayRecordCapability::Bootstrap],
            100,
            200,
            |bytes| identity.sign(bytes),
        )
        .unwrap();
        let response = RelayExchangeResponse::Relays {
            records: vec![record.clone()],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: RelayExchangeResponse = ciborium::from_reader(&buf[..]).unwrap();

        assert!(matches!(
            decoded,
            RelayExchangeResponse::Relays { records } if records == vec![record]
        ));
    }

    #[test]
    fn identity_provider_query_cbor_round_trip() {
        let identity = IdentityId::from_public_key([7; 32]);
        let request = RelayExchangeRequest::FindIdentityProviders {
            query_id: "query-1".to_string(),
            identity: identity.clone(),
            limit: 4,
            ttl: 2,
            deadline_unix_ms: 123_456,
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: RelayExchangeRequest = ciborium::from_reader(&buf[..]).unwrap();

        assert!(matches!(
            decoded,
            RelayExchangeRequest::FindIdentityProviders {
                query_id,
                identity: decoded_identity,
                limit: 4,
                ttl: 2,
                deadline_unix_ms: 123_456,
            } if query_id == "query-1" && decoded_identity == identity
        ));
    }

    #[test]
    fn identity_provider_response_cbor_round_trip() {
        let identity = IdentityId::from_public_key([8; 32]);
        let candidate = IdentityProviderCandidate {
            peer_id: "12D3KooWTest".to_string(),
            addrs: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
        };
        let response = RelayExchangeResponse::IdentityProviders {
            query_id: "query-2".to_string(),
            identity: identity.clone(),
            providers: vec![candidate.clone()],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: RelayExchangeResponse = ciborium::from_reader(&buf[..]).unwrap();

        assert!(matches!(
            decoded,
            RelayExchangeResponse::IdentityProviders {
                query_id,
                identity: decoded_identity,
                providers,
            } if query_id == "query-2"
                && decoded_identity == identity
                && providers == vec![candidate]
        ));
    }

    #[test]
    fn identity_head_gossip_cbor_round_trip() {
        let identity = NodeIdentity::generate();
        let hint = IdentityHeadHint::new(
            identity.public_key_bytes(),
            identity.identity_id(),
            "12D3KooWTest".to_string(),
            vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            Some("12D3KooWRelay".to_string()),
            7,
            UpdateLogEntryHash([3; 32]),
            100,
            200,
            |bytes| identity.sign(bytes),
        )
        .unwrap();

        let request = RelayExchangeRequest::AnnounceIdentityHeads {
            hints: vec![hint.clone()],
        };
        let mut buf = Vec::new();
        ciborium::into_writer(&request, &mut buf).unwrap();
        let decoded: RelayExchangeRequest = ciborium::from_reader(&buf[..]).unwrap();
        assert!(matches!(
            decoded,
            RelayExchangeRequest::AnnounceIdentityHeads { hints } if hints == vec![hint.clone()]
        ));

        let response = RelayExchangeResponse::IdentityHeads {
            hints: vec![hint.clone()],
        };
        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: RelayExchangeResponse = ciborium::from_reader(&buf[..]).unwrap();
        assert!(matches!(
            decoded,
            RelayExchangeResponse::IdentityHeads { hints } if hints == vec![hint]
        ));
    }
}
