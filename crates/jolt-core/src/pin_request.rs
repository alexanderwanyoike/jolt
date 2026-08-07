use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ContentId, IdentityId};

const DOMAIN_SEPARATOR: &[u8] = b"jolt:pin-request:v1\0";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PinRequestError {
    #[error("owner public key must be 32 bytes")]
    InvalidOwnerPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRequestBody {
    pub owner_public_key: Vec<u8>,
    pub content_id: ContentId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_log_content_id: Option<ContentId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_log_size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinRequest {
    pub body: PinRequestBody,
    pub signature: Vec<u8>,
}

impl PinRequest {
    pub fn new<F>(
        owner_public_key: impl Into<Vec<u8>>,
        content_id: ContentId,
        signer: F,
    ) -> Result<Self, PinRequestError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        Self::with_update_log(owner_public_key, content_id, None, signer)
    }

    pub fn with_update_log<F>(
        owner_public_key: impl Into<Vec<u8>>,
        content_id: ContentId,
        update_log_content_id: Option<ContentId>,
        signer: F,
    ) -> Result<Self, PinRequestError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = PinRequestBody {
            owner_public_key: owner_public_key.into(),
            content_id,
            update_log_content_id,
            content_size: None,
            update_log_size: None,
        };
        validate_owner_public_key(&body.owner_public_key)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }

    pub fn with_declared_sizes<F>(
        owner_public_key: impl Into<Vec<u8>>,
        content_id: ContentId,
        content_size: u64,
        update_log: Option<(ContentId, u64)>,
        signer: F,
    ) -> Result<Self, PinRequestError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let (update_log_content_id, update_log_size) = update_log
            .map(|(content_id, size)| (Some(content_id), Some(size)))
            .unwrap_or((None, None));
        let body = PinRequestBody {
            owner_public_key: owner_public_key.into(),
            content_id,
            update_log_content_id,
            content_size: Some(content_size),
            update_log_size,
        };
        validate_owner_public_key(&body.owner_public_key)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }

    pub fn verify(&self) -> Result<(), PinRequestError> {
        verify_signature(
            &self.body.owner_public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }

    pub fn owner_identity(&self) -> Result<IdentityId, PinRequestError> {
        let public_key: [u8; PUBLIC_KEY_LEN] = self
            .body
            .owner_public_key
            .clone()
            .try_into()
            .map_err(|_| PinRequestError::InvalidOwnerPublicKey)?;
        Ok(IdentityId::from_public_key(public_key))
    }
}

impl PinRequestBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_bytes(&mut bytes, &self.owner_public_key);
        put_string(&mut bytes, &self.content_id.to_string());
        match &self.update_log_content_id {
            Some(content_id) => {
                bytes.push(1);
                put_string(&mut bytes, &content_id.to_string());
            }
            None => bytes.push(0),
        }
        // Preserve the v1 bytes exactly for old requests. Size-aware requests
        // add a tagged extension so the declarations are owner-signed.
        if self.content_size.is_some() || self.update_log_size.is_some() {
            bytes.extend_from_slice(b"jolt:pin-request:sizes:v1\0");
            put_optional_u64(&mut bytes, self.content_size);
            put_optional_u64(&mut bytes, self.update_log_size);
        }
        bytes
    }
}

fn verify_signature(
    public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), PinRequestError> {
    validate_owner_public_key(public_key)?;
    validate_signature_len(signature)?;
    let key_array: [u8; PUBLIC_KEY_LEN] = public_key
        .try_into()
        .map_err(|_| PinRequestError::InvalidOwnerPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| PinRequestError::InvalidSignatureLength)?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_array).map_err(|_| PinRequestError::InvalidOwnerPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);

    use ed25519_dalek::Verifier;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| PinRequestError::InvalidSignature)
}

fn validate_owner_public_key(owner_public_key: &[u8]) -> Result<(), PinRequestError> {
    if owner_public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(PinRequestError::InvalidOwnerPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), PinRequestError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(PinRequestError::InvalidSignatureLength)
    }
}

fn put_string(bytes: &mut Vec<u8>, value: &str) {
    put_bytes(bytes, value.as_bytes());
}

fn put_bytes(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value);
}

fn put_optional_u64(bytes: &mut Vec<u8>, value: Option<u64>) {
    match value {
        Some(value) => {
            bytes.push(1);
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        None => bytes.push(0),
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    use super::*;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn public_key(key: &SigningKey) -> [u8; 32] {
        key.verifying_key().to_bytes()
    }

    fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
        key.sign(bytes).to_bytes().to_vec()
    }

    #[test]
    fn verifies_owner_signed_pin_request() {
        let owner = signing_key();
        let content_id = ContentId::from_bytes(b"content to pin");

        let request = PinRequest::new(public_key(&owner), content_id.clone(), |bytes| {
            sign(&owner, bytes)
        })
        .unwrap();

        assert_eq!(request.body.content_id, content_id);
        assert_eq!(
            request.owner_identity().unwrap(),
            IdentityId::from_public_key(public_key(&owner))
        );
        request.verify().unwrap();
    }

    #[test]
    fn rejects_request_signed_by_wrong_owner() {
        let owner = signing_key();
        let attacker = signing_key();
        let request = PinRequest::new(
            public_key(&owner),
            ContentId::from_bytes(b"content to pin"),
            |bytes| sign(&attacker, bytes),
        )
        .unwrap();

        assert_eq!(request.verify(), Err(PinRequestError::InvalidSignature));
    }

    #[test]
    fn rejects_tampered_request() {
        let owner = signing_key();
        let mut request = PinRequest::new(
            public_key(&owner),
            ContentId::from_bytes(b"content to pin"),
            |bytes| sign(&owner, bytes),
        )
        .unwrap();
        request.body.content_id = ContentId::from_bytes(b"different content");

        assert_eq!(request.verify(), Err(PinRequestError::InvalidSignature));
    }

    #[test]
    fn rejects_malformed_content_id_during_deserialization() {
        let json = serde_json::json!({
            "body": {
                "owner_public_key": vec![0; 32],
                "content_id": "not-a-cid"
            },
            "signature": vec![0; 64]
        });

        let result = serde_json::from_value::<PinRequest>(json);

        assert!(result.is_err());
    }

    #[test]
    fn serializes_over_json_wire_format() {
        let owner = signing_key();
        let request = PinRequest::with_update_log(
            public_key(&owner),
            ContentId::from_bytes(b"content to pin"),
            Some(ContentId::from_bytes(b"update log")),
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        let json = serde_json::to_string(&request).unwrap();
        let parsed: PinRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed, request);
        parsed.verify().unwrap();
    }

    #[test]
    fn declared_sizes_are_signed_and_round_trip_over_json() {
        let owner = signing_key();
        let content_id = ContentId::from_bytes(b"content to pin");
        let update_log_id = ContentId::from_bytes(b"update log");
        let mut request = PinRequest::with_declared_sizes(
            public_key(&owner),
            content_id,
            14,
            Some((update_log_id, 10)),
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        let json = serde_json::to_string(&request).unwrap();
        let parsed: PinRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.body.content_size, Some(14));
        assert_eq!(parsed.body.update_log_size, Some(10));
        parsed.verify().unwrap();

        request.body.content_size = Some(13);
        assert_eq!(request.verify(), Err(PinRequestError::InvalidSignature));
    }

    #[test]
    fn legacy_request_signature_remains_valid_without_size_fields() {
        let owner = signing_key();
        let request = PinRequest::new(
            public_key(&owner),
            ContentId::from_bytes(b"legacy content"),
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        assert_eq!(request.body.content_size, None);
        assert_eq!(request.body.update_log_size, None);
        assert!(!serde_json::to_string(&request)
            .unwrap()
            .contains("content_size"));
        request.verify().unwrap();
    }
}
