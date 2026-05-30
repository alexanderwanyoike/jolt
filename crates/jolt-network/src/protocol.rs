use serde::{Deserialize, Serialize};

use jolt_core::{IdentityId, UpdateLogEntry};

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

#[cfg(test)]
mod tests {
    use super::*;
    use jolt_core::{
        ContentId, IdentityId, RelayRecord, RelayRecordCapability, UpdateAction, UpdateLogEntry,
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
}
