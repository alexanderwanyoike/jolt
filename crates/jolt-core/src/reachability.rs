use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::IdentityId;

pub const SIGNED_REACHABILITY_PATH: &str = "/.well-known/jolt/reachability";

const DOMAIN_SEPARATOR: &[u8] = b"jolt:reachability-record:v1\0";
const RECORD_TYPE: &str = "jolt.reachability";
const RECORD_VERSION: u16 = 1;
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReachabilityRecordError {
    #[error("owner public key must be 32 bytes")]
    InvalidOwnerPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("record identity does not match owner public key")]
    IdentityMismatch,

    #[error("record type is not supported")]
    UnsupportedRecordType,

    #[error("record version is not supported")]
    UnsupportedRecordVersion,

    #[error("reachability record is expired")]
    ExpiredRecord,

    #[error("live endpoint is malformed")]
    MalformedLiveEndpoint,

    #[error("offline ingress endpoint is malformed")]
    MalformedOfflineIngressEndpoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveReachabilityEndpoint {
    pub transport: String,
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub relay_hints: Vec<String>,
    pub protocols: Vec<String>,
    pub max_payload_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfflineIngressEndpoint {
    pub transport: String,
    pub relay: String,
    pub endpoint: String,
    pub requires_invite_token: bool,
    pub max_object_bytes: u64,
    pub max_objects_per_sender_per_hour: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityRecordBody {
    pub record_type: String,
    pub version: u16,
    pub owner_public_key: Vec<u8>,
    pub identity: IdentityId,
    pub sequence_hint: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    pub live: Vec<LiveReachabilityEndpoint>,
    pub offline_ingress: Vec<OfflineIngressEndpoint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityRecord {
    pub body: ReachabilityRecordBody,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedReachability {
    pub identity: IdentityId,
    pub sequence_hint: u64,
    pub expires_at: u64,
    pub live: Vec<LiveReachabilityEndpoint>,
    pub offline_ingress: Vec<OfflineIngressEndpoint>,
}

impl ReachabilityRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new<F>(
        owner_public_key: impl Into<Vec<u8>>,
        identity: IdentityId,
        sequence_hint: u64,
        issued_at: u64,
        expires_at: u64,
        live: Vec<LiveReachabilityEndpoint>,
        offline_ingress: Vec<OfflineIngressEndpoint>,
        signer: F,
    ) -> Result<Self, ReachabilityRecordError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = ReachabilityRecordBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            owner_public_key: owner_public_key.into(),
            identity,
            sequence_hint,
            issued_at,
            expires_at,
            live,
            offline_ingress,
        };
        validate_body(&body)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl ReachabilityRecordBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.record_type);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        put_bytes(&mut bytes, &self.owner_public_key);
        put_string(&mut bytes, &self.identity.to_string());
        bytes.extend_from_slice(&self.sequence_hint.to_be_bytes());
        bytes.extend_from_slice(&self.issued_at.to_be_bytes());
        bytes.extend_from_slice(&self.expires_at.to_be_bytes());
        bytes.extend_from_slice(&(self.live.len() as u64).to_be_bytes());
        for endpoint in &self.live {
            put_string(&mut bytes, &endpoint.transport);
            put_string(&mut bytes, &endpoint.peer_id);
            put_string_vec(&mut bytes, &endpoint.addresses);
            put_string_vec(&mut bytes, &endpoint.relay_hints);
            put_string_vec(&mut bytes, &endpoint.protocols);
            bytes.extend_from_slice(&endpoint.max_payload_bytes.to_be_bytes());
        }
        bytes.extend_from_slice(&(self.offline_ingress.len() as u64).to_be_bytes());
        for endpoint in &self.offline_ingress {
            put_string(&mut bytes, &endpoint.transport);
            put_string(&mut bytes, &endpoint.relay);
            put_string(&mut bytes, &endpoint.endpoint);
            bytes.push(u8::from(endpoint.requires_invite_token));
            bytes.extend_from_slice(&endpoint.max_object_bytes.to_be_bytes());
            bytes.extend_from_slice(&endpoint.max_objects_per_sender_per_hour.to_be_bytes());
        }
        bytes
    }
}

pub fn verify_reachability_record_for_identity(
    identity: &IdentityId,
    record: &ReachabilityRecord,
    now: u64,
) -> Result<VerifiedReachability, ReachabilityRecordError> {
    validate_body(&record.body)?;
    if &record.body.identity != identity {
        return Err(ReachabilityRecordError::IdentityMismatch);
    }
    if record.body.expires_at <= now {
        return Err(ReachabilityRecordError::ExpiredRecord);
    }
    verify_signature(
        &record.body.owner_public_key,
        &record.body.canonical_bytes(),
        &record.signature,
    )?;

    Ok(VerifiedReachability {
        identity: record.body.identity.clone(),
        sequence_hint: record.body.sequence_hint,
        expires_at: record.body.expires_at,
        live: record.body.live.clone(),
        offline_ingress: record.body.offline_ingress.clone(),
    })
}

fn validate_body(body: &ReachabilityRecordBody) -> Result<(), ReachabilityRecordError> {
    if body.record_type != RECORD_TYPE {
        return Err(ReachabilityRecordError::UnsupportedRecordType);
    }
    if body.version != RECORD_VERSION {
        return Err(ReachabilityRecordError::UnsupportedRecordVersion);
    }
    validate_owner_public_key(&body.owner_public_key)?;
    let owner_identity = IdentityId::from_public_key(
        body.owner_public_key
            .as_slice()
            .try_into()
            .map_err(|_| ReachabilityRecordError::InvalidOwnerPublicKey)?,
    );
    if owner_identity != body.identity {
        return Err(ReachabilityRecordError::IdentityMismatch);
    }
    if body.expires_at <= body.issued_at {
        return Err(ReachabilityRecordError::ExpiredRecord);
    }
    for endpoint in &body.live {
        validate_live_endpoint(endpoint)?;
    }
    for endpoint in &body.offline_ingress {
        validate_offline_ingress_endpoint(endpoint)?;
    }
    Ok(())
}

fn validate_live_endpoint(
    endpoint: &LiveReachabilityEndpoint,
) -> Result<(), ReachabilityRecordError> {
    if endpoint.transport.trim().is_empty()
        || endpoint.peer_id.trim().is_empty()
        || endpoint.protocols.is_empty()
        || endpoint
            .protocols
            .iter()
            .any(|protocol| protocol.trim().is_empty())
        || endpoint.max_payload_bytes == 0
    {
        return Err(ReachabilityRecordError::MalformedLiveEndpoint);
    }
    Ok(())
}

fn validate_offline_ingress_endpoint(
    endpoint: &OfflineIngressEndpoint,
) -> Result<(), ReachabilityRecordError> {
    if endpoint.transport.trim().is_empty()
        || endpoint.relay.trim().is_empty()
        || endpoint.endpoint.trim().is_empty()
        || endpoint.max_object_bytes == 0
        || endpoint.max_objects_per_sender_per_hour == 0
    {
        return Err(ReachabilityRecordError::MalformedOfflineIngressEndpoint);
    }
    Ok(())
}

fn verify_signature(
    owner_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), ReachabilityRecordError> {
    validate_owner_public_key(owner_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = owner_public_key
        .try_into()
        .map_err(|_| ReachabilityRecordError::InvalidOwnerPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| ReachabilityRecordError::InvalidSignatureLength)?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| ReachabilityRecordError::InvalidOwnerPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);
    key.verify_strict(payload, &signature)
        .map_err(|_| ReachabilityRecordError::InvalidSignature)
}

fn validate_owner_public_key(public_key: &[u8]) -> Result<(), ReachabilityRecordError> {
    if public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(ReachabilityRecordError::InvalidOwnerPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), ReachabilityRecordError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(ReachabilityRecordError::InvalidSignatureLength)
    }
}

fn put_string_vec(bytes: &mut Vec<u8>, values: &[String]) {
    bytes.extend_from_slice(&(values.len() as u64).to_be_bytes());
    for value in values {
        put_string(bytes, value);
    }
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    put_bytes(bytes, value.as_bytes());
}

fn put_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use crate::{
        verify_reachability_record_for_identity, IdentityId, LiveReachabilityEndpoint,
        OfflineIngressEndpoint, ReachabilityRecord, ReachabilityRecordError,
    };

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn live_endpoint() -> LiveReachabilityEndpoint {
        LiveReachabilityEndpoint {
            transport: "jolt-libp2p-stream".to_string(),
            peer_id: "12D3KooWReachablePeer".to_string(),
            addresses: vec!["/ip4/127.0.0.1/udp/4100/quic-v1".to_string()],
            relay_hints: vec!["relay_identity".to_string()],
            protocols: vec!["opaque-app-stream-v1".to_string()],
            max_payload_bytes: 65_536,
        }
    }

    fn offline_endpoint() -> OfflineIngressEndpoint {
        OfflineIngressEndpoint {
            transport: "jolt-object-ingress-v1".to_string(),
            relay: "relay_identity".to_string(),
            endpoint: "ingress_abc".to_string(),
            requires_invite_token: true,
            max_object_bytes: 65_536,
            max_objects_per_sender_per_hour: 20,
        }
    }

    fn record_for(owner: &SigningKey) -> ReachabilityRecord {
        let public_key = owner.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        ReachabilityRecord::new(
            public_key,
            identity,
            42,
            100,
            200,
            vec![live_endpoint()],
            vec![offline_endpoint()],
            |bytes| owner.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap()
    }

    #[test]
    fn verifies_owner_signed_reachability_record() {
        let owner = key(7);
        let identity = IdentityId::from_public_key(owner.verifying_key().to_bytes());
        let record = record_for(&owner);

        let verified = verify_reachability_record_for_identity(&identity, &record, 150).unwrap();

        assert_eq!(verified.identity, identity);
        assert_eq!(verified.sequence_hint, 42);
        assert_eq!(verified.live.len(), 1);
        assert_eq!(verified.offline_ingress.len(), 1);
        assert_eq!(verified.live[0].transport, "jolt-libp2p-stream");
    }

    #[test]
    fn rejects_records_for_a_different_identity() {
        let owner = key(7);
        let other = key(8);
        let record = record_for(&owner);
        let other_identity = IdentityId::from_public_key(other.verifying_key().to_bytes());

        assert_eq!(
            verify_reachability_record_for_identity(&other_identity, &record, 150),
            Err(ReachabilityRecordError::IdentityMismatch)
        );
    }

    #[test]
    fn rejects_tampered_records() {
        let owner = key(7);
        let identity = IdentityId::from_public_key(owner.verifying_key().to_bytes());
        let mut record = record_for(&owner);
        record.body.live[0].transport = "tampered".to_string();

        assert_eq!(
            verify_reachability_record_for_identity(&identity, &record, 150),
            Err(ReachabilityRecordError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_expired_records() {
        let owner = key(7);
        let identity = IdentityId::from_public_key(owner.verifying_key().to_bytes());
        let record = record_for(&owner);

        assert_eq!(
            verify_reachability_record_for_identity(&identity, &record, 200),
            Err(ReachabilityRecordError::ExpiredRecord)
        );
    }

    #[test]
    fn rejects_malformed_live_endpoints() {
        let owner = key(7);
        let public_key = owner.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        let mut endpoint = live_endpoint();
        endpoint.protocols.clear();

        assert_eq!(
            ReachabilityRecord::new(
                public_key,
                identity,
                42,
                100,
                200,
                vec![endpoint],
                vec![],
                |bytes| owner.sign(bytes).to_bytes().to_vec(),
            ),
            Err(ReachabilityRecordError::MalformedLiveEndpoint)
        );
    }

    #[test]
    fn rejects_malformed_offline_ingress_endpoints() {
        let owner = key(7);
        let public_key = owner.verifying_key().to_bytes();
        let identity = IdentityId::from_public_key(public_key);
        let mut endpoint = offline_endpoint();
        endpoint.endpoint.clear();

        assert_eq!(
            ReachabilityRecord::new(
                public_key,
                identity,
                42,
                100,
                200,
                vec![],
                vec![endpoint],
                |bytes| owner.sign(bytes).to_bytes().to_vec(),
            ),
            Err(ReachabilityRecordError::MalformedOfflineIngressEndpoint)
        );
    }
}
