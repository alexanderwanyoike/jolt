use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{content_id::ContentId, IdentityId, JoltAddress};

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

    #[error("entry at index {index} changes owner public key")]
    OwnerChanged { index: usize },

    #[error("cannot resolve latest state from an empty update log")]
    EmptyLog,

    #[error("requested address identity does not match update log owner")]
    IdentityMismatch,

    #[error("no content path found for {path}")]
    PathNotFound { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UpdateLogEntryHash(pub [u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateAction {
    PublishContent { content_id: ContentId },
    UpdateRoot { content_id: ContentId },
    UpdateProfile { profile: UpdateProfile },
    SetPath { path: String, content_id: ContentId },
    RemovePath { path: String },
    SetReachability { relays: Vec<RelayHint> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateProfile {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub avatar: Option<ContentId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayCapability {
    Discovery,
    Pinning,
    Serving,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelayHint {
    pub identity: IdentityId,
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub capabilities: Vec<RelayCapability>,
    pub expires_at: Option<u64>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLatestRecord {
    pub owner_public_key: Vec<u8>,
    pub latest_sequence: u64,
    pub latest_root: Option<ContentId>,
    pub profile: Option<UpdateProfile>,
    pub paths: BTreeMap<String, ContentId>,
    pub reachability: Vec<RelayHint>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedJoltTarget {
    pub identity: IdentityId,
    pub path: String,
    pub content_id: ContentId,
    pub reachability: Vec<RelayHint>,
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
            Self::SetPath { path, content_id } => {
                bytes.push(3);
                put_string(bytes, path);
                put_string(bytes, &content_id.to_string());
            }
            Self::RemovePath { path } => {
                bytes.push(4);
                put_string(bytes, path);
            }
            Self::SetReachability { relays } => {
                bytes.push(5);
                bytes.extend_from_slice(&(relays.len() as u64).to_be_bytes());
                for relay in relays {
                    relay.encode(bytes);
                }
            }
        }
    }
}

impl RelayHint {
    fn encode(&self, bytes: &mut Vec<u8>) {
        put_string(bytes, &self.identity.to_string());
        put_string(bytes, &self.peer_id);
        bytes.extend_from_slice(&(self.addresses.len() as u64).to_be_bytes());
        for address in &self.addresses {
            put_string(bytes, address);
        }
        bytes.extend_from_slice(&(self.capabilities.len() as u64).to_be_bytes());
        for capability in &self.capabilities {
            bytes.push(match capability {
                RelayCapability::Discovery => 0,
                RelayCapability::Pinning => 1,
                RelayCapability::Serving => 2,
            });
        }
        match self.expires_at {
            Some(expires_at) => {
                bytes.push(1);
                bytes.extend_from_slice(&expires_at.to_be_bytes());
            }
            None => bytes.push(0),
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

        if entry.body.owner_public_key != entries[0].body.owner_public_key {
            return Err(UpdateLogError::OwnerChanged { index });
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

pub fn resolve_latest_record(
    entries: &[UpdateLogEntry],
) -> Result<ResolvedLatestRecord, UpdateLogError> {
    verify_update_log(entries)?;

    let Some(first) = entries.first() else {
        return Err(UpdateLogError::EmptyLog);
    };

    let mut resolved = ResolvedLatestRecord {
        owner_public_key: first.body.owner_public_key.clone(),
        latest_sequence: first.body.sequence,
        latest_root: None,
        profile: None,
        paths: BTreeMap::new(),
        reachability: Vec::new(),
    };

    for entry in entries {
        resolved.latest_sequence = entry.body.sequence;
        match &entry.body.action {
            UpdateAction::PublishContent { .. } => {}
            UpdateAction::UpdateRoot { content_id } => {
                resolved.latest_root = Some(content_id.clone());
            }
            UpdateAction::UpdateProfile { profile } => {
                resolved.profile = Some(profile.clone());
            }
            UpdateAction::SetPath { path, content_id } => {
                resolved.paths.insert(path.clone(), content_id.clone());
            }
            UpdateAction::RemovePath { path } => {
                resolved.paths.remove(path);
            }
            UpdateAction::SetReachability { relays } => {
                resolved.reachability = relays.clone();
            }
        }
    }

    Ok(resolved)
}

pub fn resolve_jolt_address(
    address: &JoltAddress,
    entries: &[UpdateLogEntry],
    now: Option<u64>,
) -> Result<ResolvedJoltTarget, UpdateLogError> {
    let resolved = resolve_latest_record(entries)?;
    let identity = IdentityId::from_public_key(
        resolved
            .owner_public_key
            .as_slice()
            .try_into()
            .map_err(|_| UpdateLogError::InvalidOwnerPublicKey)?,
    );

    if address.identity() != &identity {
        return Err(UpdateLogError::IdentityMismatch);
    }

    let path = address.path().to_string();
    let content_id = resolved
        .paths
        .get(address.path())
        .cloned()
        .ok_or_else(|| UpdateLogError::PathNotFound { path: path.clone() })?;
    let reachability = match now {
        Some(now) => resolved
            .reachability
            .into_iter()
            .filter(|relay| {
                relay
                    .expires_at
                    .map(|expires_at| expires_at > now)
                    .unwrap_or(true)
            })
            .collect(),
        None => resolved.reachability,
    };

    Ok(ResolvedJoltTarget {
        identity,
        path,
        content_id,
        reachability,
    })
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

    fn identity_id(key: &SigningKey) -> IdentityId {
        IdentityId::from_public_key(public_key(key))
    }

    fn jolt_address(key: &SigningKey, path: &str) -> JoltAddress {
        JoltAddress::new(identity_id(key), path).unwrap()
    }

    fn content_id(label: &[u8]) -> ContentId {
        ContentId::from_bytes(label)
    }

    fn relay_hint(key: &SigningKey, expires_at: Option<u64>) -> RelayHint {
        RelayHint {
            identity: identity_id(key),
            peer_id: "12D3KooWRelayPeer".to_string(),
            addresses: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
            capabilities: vec![RelayCapability::Discovery, RelayCapability::Serving],
            expires_at,
        }
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

    #[test]
    fn resolves_latest_profile_state() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::UpdateProfile {
                profile: UpdateProfile {
                    display_name: Some("Alice".to_string()),
                    bio: None,
                    avatar: None,
                },
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();
        let latest_profile = genesis
            .append(
                UpdateAction::UpdateProfile {
                    profile: UpdateProfile {
                        display_name: Some("Alice A.".to_string()),
                        bio: Some("Building Jolt".to_string()),
                        avatar: Some(content_id(b"avatar")),
                    },
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        let resolved = resolve_latest_record(&[genesis, latest_profile]).unwrap();

        assert_eq!(
            resolved.profile,
            Some(UpdateProfile {
                display_name: Some("Alice A.".to_string()),
                bio: Some("Building Jolt".to_string()),
                avatar: Some(content_id(b"avatar")),
            })
        );
    }

    #[test]
    fn resolves_newest_content_for_repeated_path_updates() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::SetPath {
                path: "/posts/hello".to_string(),
                content_id: content_id(b"first version"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();
        let updated = genesis
            .append(
                UpdateAction::SetPath {
                    path: "/posts/hello".to_string(),
                    content_id: content_id(b"second version"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        let resolved = resolve_latest_record(&[genesis, updated]).unwrap();

        assert_eq!(
            resolved.paths.get("/posts/hello"),
            Some(&content_id(b"second version"))
        );
    }

    #[test]
    fn omits_removed_paths() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::SetPath {
                path: "/posts/hello".to_string(),
                content_id: content_id(b"hello"),
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();
        let removed = genesis
            .append(
                UpdateAction::RemovePath {
                    path: "/posts/hello".to_string(),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        let resolved = resolve_latest_record(&[genesis, removed]).unwrap();

        assert!(!resolved.paths.contains_key("/posts/hello"));
    }

    #[test]
    fn resolver_rejects_invalid_logs() {
        let owner = signing_key();
        let attacker = signing_key();
        let entry = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::UpdateRoot {
                content_id: content_id(b"root"),
            },
            |bytes| sign(&attacker, bytes),
        )
        .unwrap();

        assert_eq!(
            resolve_latest_record(&[entry]),
            Err(UpdateLogError::InvalidSignature)
        );
    }

    #[test]
    fn resolver_rejects_owner_changes() {
        let owner = signing_key();
        let attacker = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::UpdateRoot {
                content_id: content_id(b"root"),
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        let mut next = genesis
            .append(
                UpdateAction::SetPath {
                    path: "/posts/hello".to_string(),
                    content_id: content_id(b"hello"),
                },
                |bytes| sign(&owner, bytes),
            )
            .unwrap();
        next.body.owner_public_key = public_key(&attacker).to_vec();
        next.signature = sign(&attacker, &next.body.canonical_bytes());

        assert_eq!(
            resolve_latest_record(&[genesis, next]),
            Err(UpdateLogError::OwnerChanged { index: 1 })
        );
    }

    #[test]
    fn resolver_rejects_empty_logs() {
        assert_eq!(resolve_latest_record(&[]), Err(UpdateLogError::EmptyLog));
    }

    #[test]
    fn resolver_replays_realistic_log_into_current_state() {
        let key = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&key),
            UpdateAction::UpdateProfile {
                profile: UpdateProfile {
                    display_name: Some("Alice".to_string()),
                    bio: Some("Early profile".to_string()),
                    avatar: None,
                },
            },
            |bytes| sign(&key, bytes),
        )
        .unwrap();
        let root = genesis
            .append(
                UpdateAction::UpdateRoot {
                    content_id: content_id(b"site root v1"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        let post_v1 = root
            .append(
                UpdateAction::SetPath {
                    path: "/posts/hello".to_string(),
                    content_id: content_id(b"hello v1"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        let about = post_v1
            .append(
                UpdateAction::SetPath {
                    path: "/about".to_string(),
                    content_id: content_id(b"about"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        let post_v2 = about
            .append(
                UpdateAction::SetPath {
                    path: "/posts/hello".to_string(),
                    content_id: content_id(b"hello v2"),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();
        let remove_about = post_v2
            .append(
                UpdateAction::RemovePath {
                    path: "/about".to_string(),
                },
                |bytes| sign(&key, bytes),
            )
            .unwrap();

        let resolved =
            resolve_latest_record(&[genesis, root, post_v1, about, post_v2, remove_about]).unwrap();

        assert_eq!(resolved.latest_sequence, 5);
        assert_eq!(resolved.latest_root, Some(content_id(b"site root v1")));
        assert_eq!(
            resolved.paths.get("/posts/hello"),
            Some(&content_id(b"hello v2"))
        );
        assert!(!resolved.paths.contains_key("/about"));
        assert_eq!(
            resolved.profile,
            Some(UpdateProfile {
                display_name: Some("Alice".to_string()),
                bio: Some("Early profile".to_string()),
                avatar: None,
            })
        );
    }

    #[test]
    fn latest_record_replays_signed_reachability() {
        let owner = signing_key();
        let relay_a = signing_key();
        let relay_b = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetReachability {
                relays: vec![relay_hint(&relay_a, None)],
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();
        let updated = genesis
            .append(
                UpdateAction::SetReachability {
                    relays: vec![relay_hint(&relay_b, Some(200))],
                },
                |bytes| sign(&owner, bytes),
            )
            .unwrap();

        let resolved = resolve_latest_record(&[genesis, updated]).unwrap();

        assert_eq!(resolved.reachability, vec![relay_hint(&relay_b, Some(200))]);
    }

    #[test]
    fn resolves_jolt_address_to_content_and_reachability() {
        let owner = signing_key();
        let relay = signing_key();
        let content = content_id(b"profile content");
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: content.clone(),
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();
        let reachable = genesis
            .append(
                UpdateAction::SetReachability {
                    relays: vec![relay_hint(&relay, None)],
                },
                |bytes| sign(&owner, bytes),
            )
            .unwrap();

        let target = resolve_jolt_address(
            &jolt_address(&owner, "/profile"),
            &[genesis, reachable],
            None,
        )
        .unwrap();

        assert_eq!(target.identity, identity_id(&owner));
        assert_eq!(target.path, "/profile");
        assert_eq!(target.content_id, content);
        assert_eq!(target.reachability, vec![relay_hint(&relay, None)]);
    }

    #[test]
    fn jolt_resolver_rejects_identity_mismatch() {
        let owner = signing_key();
        let other = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: content_id(b"profile"),
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        assert_eq!(
            resolve_jolt_address(&jolt_address(&other, "/profile"), &[genesis], None),
            Err(UpdateLogError::IdentityMismatch)
        );
    }

    #[test]
    fn jolt_resolver_rejects_invalid_signatures() {
        let owner = signing_key();
        let attacker = signing_key();
        let entry = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: content_id(b"profile"),
            },
            |bytes| sign(&attacker, bytes),
        )
        .unwrap();

        assert_eq!(
            resolve_jolt_address(&jolt_address(&owner, "/profile"), &[entry], None),
            Err(UpdateLogError::InvalidSignature)
        );
    }

    #[test]
    fn jolt_resolver_rejects_missing_path() {
        let owner = signing_key();
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: content_id(b"profile"),
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();

        assert_eq!(
            resolve_jolt_address(&jolt_address(&owner, "/missing"), &[genesis], None),
            Err(UpdateLogError::PathNotFound {
                path: "/missing".to_string()
            })
        );
    }

    #[test]
    fn jolt_resolver_filters_expired_reachability_hints() {
        let owner = signing_key();
        let expired = signing_key();
        let current = signing_key();
        let content = content_id(b"profile");
        let genesis = UpdateLogEntry::genesis(
            public_key(&owner),
            UpdateAction::SetPath {
                path: "/profile".to_string(),
                content_id: content.clone(),
            },
            |bytes| sign(&owner, bytes),
        )
        .unwrap();
        let reachable = genesis
            .append(
                UpdateAction::SetReachability {
                    relays: vec![
                        relay_hint(&expired, Some(100)),
                        relay_hint(&current, Some(300)),
                    ],
                },
                |bytes| sign(&owner, bytes),
            )
            .unwrap();

        let target = resolve_jolt_address(
            &jolt_address(&owner, "/profile"),
            &[genesis, reachable],
            Some(200),
        )
        .unwrap();

        assert_eq!(target.content_id, content);
        assert_eq!(target.reachability, vec![relay_hint(&current, Some(300))]);
    }
}
