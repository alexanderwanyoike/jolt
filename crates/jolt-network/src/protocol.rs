use serde::{Deserialize, Serialize};

use jolt_core::{IdentityHeadHint, IdentityId, RelayRecord, RelayRecordCapability, UpdateLogEntry};

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
