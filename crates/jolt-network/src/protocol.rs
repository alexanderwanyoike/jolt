use serde::{Deserialize, Serialize};

use jolt_core::{
    DeviceAuthorizationRecord, DeviceWriterLogEntry, IdentityHeadHint, IdentityId, RelayRecord,
    RelayRecordCapability, UpdateLogEntry,
};

use crate::command::IngressRecord;

pub const LEGACY_DEVICE_WRITER_OPERATION_VERSION: u16 = 1;
pub const TOMBSTONE_DEVICE_WRITER_OPERATION_VERSION: u16 = 2;
pub const CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION: u16 = 3;

fn legacy_device_writer_operation_version() -> u16 {
    LEGACY_DEVICE_WRITER_OPERATION_VERSION
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
}

impl DeviceWriterSyncRequest {
    pub fn new(identity: IdentityId) -> Self {
        Self {
            identity,
            max_operation_version: CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION,
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
    pub authority_records: Vec<DeviceAuthorizationRecord>,
    pub device_logs: Vec<Vec<DeviceWriterLogEntry>>,
}

impl DeviceWriterSyncResponse {
    pub fn unsupported_operation_version(required_operation_version: u16) -> Self {
        Self {
            required_operation_version,
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
            authority_records,
            device_logs,
        }
    }

    pub fn ensure_supported(
        &self,
        supported_operation_version: u16,
    ) -> Result<(), crate::error::NetworkError> {
        if self.required_operation_version > supported_operation_version {
            Err(
                crate::error::NetworkError::UnsupportedDeviceWriterOperationVersion {
                    supported: supported_operation_version,
                    required: self.required_operation_version,
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
            authority_records: authority.clone(),
            device_logs: vec![device_log.clone()],
        };

        let mut buf = Vec::new();
        ciborium::into_writer(&response, &mut buf).unwrap();
        let decoded: DeviceWriterSyncResponse = ciborium::from_reader(&buf[..]).unwrap();

        assert_eq!(decoded.authority_records, authority);
        assert_eq!(decoded.device_logs, vec![device_log]);
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
            .ensure_supported(CAUSAL_HEADS_DEVICE_WRITER_OPERATION_VERSION)
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
