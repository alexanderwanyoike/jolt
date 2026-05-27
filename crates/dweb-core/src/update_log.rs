use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::content_id::ContentId;

const DOMAIN_SEPARATOR: &[u8] = b"jolt:update-log-entry:v1\0";
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateLogError {
    #[error("owner public key must be 32 bytes")]
    InvalidOwnerPublicKey,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("genesis entry must have sequence 0")]
    InvalidGenesisSequence,

    #[error("genesis entry must not have a previous entry hash")]
    GenesisHasPreviousHash,

    #[error("entry at index {index} has sequence {actual}, expected {expected}")]
    OutOfOrderSequence {
        index: usize,
        expected: u64,
        actual: u64,
    },

    #[error("entry at index {index} has a broken previous-entry hash")]
    BrokenPreviousHash { index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UpdateLogEntryHash(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateAction {
    PublishContent { content_id: ContentId },
    UpdateRoot { content_id: ContentId },
    UpdateProfile { profile: UpdateProfile },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProfile {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar: Option<ContentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateLogEntryBody {
    pub owner_public_key: Vec<u8>,
    pub sequence: u64,
    pub previous_entry_hash: Option<UpdateLogEntryHash>,
    pub action: UpdateAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateLogEntry {
    pub body: UpdateLogEntryBody,
    pub signature: Vec<u8>,
}

impl UpdateLogEntry {
    pub fn genesis<F>(
        owner_public_key: impl Into<Vec<u8>>,
        action: UpdateAction,
        signer: F,
    ) -> Result<Self, UpdateLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = UpdateLogEntryBody {
            owner_public_key: owner_public_key.into(),
            sequence: 0,
            previous_entry_hash: None,
            action,
        };
        Self::sign_body(body, signer)
    }

    pub fn append<F>(&self, action: UpdateAction, signer: F) -> Result<Self, UpdateLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = UpdateLogEntryBody {
            owner_public_key: self.body.owner_public_key.clone(),
            sequence: self.body.sequence + 1,
            previous_entry_hash: Some(self.entry_hash()),
            action,
        };
        Self::sign_body(body, signer)
    }

    pub fn entry_hash(&self) -> UpdateLogEntryHash {
        UpdateLogEntryHash(*blake3::hash(&self.body.canonical_bytes()).as_bytes())
    }

    pub fn verify_signature(&self) -> Result<(), UpdateLogError> {
        verify_signature(
            &self.body.owner_public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }

    fn sign_body<F>(body: UpdateLogEntryBody, signer: F) -> Result<Self, UpdateLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        validate_owner_public_key(&body.owner_public_key)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl UpdateLogEntryBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_bytes(&mut bytes, &self.owner_public_key);
        bytes.extend_from_slice(&self.sequence.to_be_bytes());
        match &self.previous_entry_hash {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&hash.0);
            }
            None => bytes.push(0),
        }
        self.action.encode(&mut bytes);
        bytes
    }
}

impl UpdateAction {
    fn encode(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::PublishContent { content_id } => {
                bytes.push(0);
                put_string(bytes, &content_id.to_string());
            }
            Self::UpdateRoot { content_id } => {
                bytes.push(1);
                put_string(bytes, &content_id.to_string());
            }
            Self::UpdateProfile { profile } => {
                bytes.push(2);
                put_optional_string(bytes, profile.display_name.as_deref());
                put_optional_string(bytes, profile.bio.as_deref());
                let avatar = profile.avatar.as_ref().map(ToString::to_string);
                put_optional_string(bytes, avatar.as_deref());
            }
        }
    }
}

pub fn verify_update_log(entries: &[UpdateLogEntry]) -> Result<(), UpdateLogError> {
    for (index, entry) in entries.iter().enumerate() {
        entry.verify_signature()?;

        if index == 0 {
            if entry.body.sequence != 0 {
                return Err(UpdateLogError::InvalidGenesisSequence);
            }
            if entry.body.previous_entry_hash.is_some() {
                return Err(UpdateLogError::GenesisHasPreviousHash);
            }
            continue;
        }

        let expected_sequence = entries[index - 1].body.sequence + 1;
        if entry.body.sequence != expected_sequence {
            return Err(UpdateLogError::OutOfOrderSequence {
                index,
                expected: expected_sequence,
                actual: entry.body.sequence,
            });
        }

        let expected_previous_hash = entries[index - 1].entry_hash();
        if entry.body.previous_entry_hash.as_ref() != Some(&expected_previous_hash) {
            return Err(UpdateLogError::BrokenPreviousHash { index });
        }
    }

    Ok(())
}

fn verify_signature(
    owner_public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), UpdateLogError> {
    validate_owner_public_key(owner_public_key)?;
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = owner_public_key
        .try_into()
        .map_err(|_| UpdateLogError::InvalidOwnerPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| UpdateLogError::InvalidSignatureLength)?;

    let verifying_key =
        VerifyingKey::from_bytes(&key_array).map_err(|_| UpdateLogError::InvalidOwnerPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);

    use ed25519_dalek::Verifier;
    verifying_key
        .verify(payload, &signature)
        .map_err(|_| UpdateLogError::InvalidSignature)
}

fn validate_owner_public_key(owner_public_key: &[u8]) -> Result<(), UpdateLogError> {
    if owner_public_key.len() == PUBLIC_KEY_LEN {
        Ok(())
    } else {
        Err(UpdateLogError::InvalidOwnerPublicKey)
    }
}

fn validate_signature_len(signature: &[u8]) -> Result<(), UpdateLogError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(UpdateLogError::InvalidSignatureLength)
    }
}

fn put_optional_string(bytes: &mut Vec<u8>, value: Option<&str>) {
    match value {
        Some(value) => {
            bytes.push(1);
            put_string(bytes, value);
        }
        None => bytes.push(0),
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
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn sign(key: &SigningKey, bytes: &[u8]) -> Vec<u8> {
        use ed25519_dalek::Signer;
        key.sign(bytes).to_bytes().to_vec()
    }

    fn public_key(key: &SigningKey) -> [u8; 32] {
        key.verifying_key().to_bytes()
    }

    fn content_id(label: &[u8]) -> ContentId {
        ContentId::from_bytes(label)
    }

    #[test]
    fn creates_and_verifies_genesis_entry() {
        let key = signing_key();

        let entry = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();

        assert_eq!(entry.body.sequence, 0);
        assert!(entry.body.previous_entry_hash.is_none());
        verify_update_log(&[entry]).unwrap();
    }

    #[test]
    fn appends_signed_entry() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();

        let next = genesis
            .append(
                UpdateAction::UpdateRoot {
                    content_id: content_id(b"new root"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        assert_eq!(next.body.sequence, 1);
        assert_eq!(next.body.previous_entry_hash, Some(genesis.entry_hash()));
        verify_update_log(&[genesis, next]).unwrap();
    }

    #[test]
    fn rejects_entries_signed_by_wrong_key() {
        let owner = signing_key();
        let attacker = signing_key();

        let entry = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&attacker, bytes),
        )
        .unwrap();

        assert_eq!(
            verify_update_log(&[entry]),
            Err(UpdateLogError::InvalidSignature)
        );
    }

    #[test]
    fn rejects_broken_previous_entry_hashes() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();

        let mut next = genesis
            .append(
                UpdateAction::UpdateRoot {
                    content_id: content_id(b"new root"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        next.body.previous_entry_hash = Some(UpdateLogEntryHash([7; 32]));
        next.signature = sign(&key, &next.body.canonical_bytes());

        assert_eq!(
            verify_update_log(&[genesis, next]),
            Err(UpdateLogError::BrokenPreviousHash { index: 1 })
        );
    }

    #[test]
    fn rejects_out_of_order_sequence_numbers() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();

        let mut next = genesis
            .append(
                UpdateAction::UpdateRoot {
                    content_id: content_id(b"new root"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        next.body.sequence = 3;
        next.signature = sign(&key, &next.body.canonical_bytes());

        assert_eq!(
            verify_update_log(&[genesis, next]),
            Err(UpdateLogError::OutOfOrderSequence {
                index: 1,
                expected: 1,
                actual: 3
            })
        );
    }

    #[test]
    fn covers_publish_root_and_profile_actions() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::PublishContent {
                content_id: content_id(b"first post"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();
        let root = genesis
            .append(
                UpdateAction::UpdateRoot {
                    content_id: content_id(b"site root"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        let profile = root
            .append(
                UpdateAction::UpdateProfile {
                    profile: UpdateProfile {
                        display_name: Some("Alice".to_string()),
                        bio: Some("Jolt publisher".to_string()),
                        avatar: Some(content_id(b"avatar image")),
                    },
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        verify_update_log(&[genesis, root, profile]).unwrap();
    }
}
