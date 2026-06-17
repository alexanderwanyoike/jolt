use std::collections::BTreeMap;

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    AuthorizedDeviceStatus, ContentId, IdentityId, JoltAddress, ResolvedJoltTarget,
    VerifiedIdentityAuthority,
};

const DOMAIN_SEPARATOR: &[u8] = b"jolt:device-writer-log-entry:v1\0";
const RECORD_TYPE: &str = "jolt.device_writer_log_entry";
const RECORD_VERSION: u16 = 1;
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DeviceWriterLogError {
    #[error("device writer log entry type is not supported")]
    UnsupportedEntryType,

    #[error("device writer log entry version is not supported")]
    UnsupportedEntryVersion,

    #[error("device writer log has an entry for a different identity")]
    IdentityMismatch,

    #[error("device id is required")]
    EmptyDeviceId,

    #[error("device writer log entry signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid device writer log entry signature")]
    InvalidSignature,

    #[error("device writer log entry at index {index} changes device id")]
    DeviceChanged { index: usize },

    #[error("device writer log entry at index {index} has sequence {actual}, expected {expected}")]
    OutOfOrderSequence {
        index: usize,
        expected: u64,
        actual: u64,
    },

    #[error("device writer log genesis entry must have sequence 0")]
    InvalidGenesisSequence,

    #[error("device writer log genesis entry must not have a previous entry hash")]
    GenesisHasPreviousHash,

    #[error("device writer log entry at index {index} has a broken previous-entry hash")]
    BrokenPreviousHash { index: usize },

    #[error("singleton path is required")]
    EmptyPath,

    #[error("device writer path is invalid")]
    InvalidPath,

    #[error("requested address identity does not match merged device state")]
    ResolveIdentityMismatch,

    #[error("no merged singleton path found for {path}")]
    PathNotFound { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceWriterLogEntryHash(pub [u8; 32]);

impl DeviceWriterLogEntryHash {
    /// Lowercase hex rendering, for stable identification across API boundaries.
    pub fn to_hex(&self) -> String {
        blake3::Hash::from_bytes(self.0).to_hex().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceWriterPathMode {
    Singleton,
    Append,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceWriterOperation {
    SetPath {
        path: String,
        content_id: ContentId,
        mode: DeviceWriterPathMode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceWriterLogEntryBody {
    pub record_type: String,
    pub version: u16,
    pub identity: IdentityId,
    pub device_id: String,
    pub device_sequence: u64,
    pub previous_entry_hash: Option<DeviceWriterLogEntryHash>,
    pub operation: DeviceWriterOperation,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceWriterLogEntry {
    pub body: DeviceWriterLogEntryBody,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedDeviceIdentityState {
    pub identity: IdentityId,
    pub singleton_paths: BTreeMap<String, MergedDeviceWriterEntry>,
    pub singleton_conflicts: BTreeMap<String, Vec<MergedDeviceWriterEntry>>,
    pub append_records: BTreeMap<String, Vec<MergedDeviceWriterEntry>>,
    pub rejected_entries: Vec<RejectedDeviceWriterEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedDeviceWriterEntry {
    pub identity: IdentityId,
    pub device_id: String,
    pub device_sequence: u64,
    pub entry_hash: DeviceWriterLogEntryHash,
    pub created_at: u64,
    pub content_id: ContentId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedDeviceWriterEntry {
    pub identity: IdentityId,
    pub device_id: String,
    pub device_sequence: u64,
    pub entry_hash: DeviceWriterLogEntryHash,
    pub content_id: ContentId,
    pub reason: DeviceWriterRejectionReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceWriterRejectionReason {
    UnknownDevice,
    RevokedDevice,
}

impl MergedDeviceIdentityState {
    /// Enumerate append records whose path starts with `prefix`, paired with
    /// their path, in deterministic order (paths ascending, then per-path merge
    /// order). This is the read seam a Collection is assembled from.
    pub fn append_records_under(&self, prefix: &str) -> Vec<(&str, &MergedDeviceWriterEntry)> {
        self.append_records
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .flat_map(|(path, entries)| entries.iter().map(move |entry| (path.as_str(), entry)))
            .collect()
    }
}

impl DeviceWriterOperation {
    pub fn set_path(
        path: impl Into<String>,
        content_id: ContentId,
        mode: DeviceWriterPathMode,
    ) -> Self {
        Self::SetPath {
            path: path.into(),
            content_id,
            mode,
        }
    }

    pub fn append_record(path: impl Into<String>, content_id: ContentId) -> Self {
        Self::set_path(path, content_id, DeviceWriterPathMode::Append)
    }

    fn encode(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::SetPath {
                path,
                content_id,
                mode,
            } => {
                bytes.push(0);
                put_string(bytes, path);
                put_string(bytes, &content_id.to_string());
                bytes.push(match mode {
                    DeviceWriterPathMode::Singleton => 0,
                    DeviceWriterPathMode::Append => 1,
                });
            }
        }
    }
}

impl DeviceWriterLogEntry {
    pub fn genesis<F>(
        identity: IdentityId,
        device_id: impl Into<String>,
        operation: DeviceWriterOperation,
        created_at: u64,
        signer: F,
    ) -> Result<Self, DeviceWriterLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = DeviceWriterLogEntryBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            identity,
            device_id: device_id.into(),
            device_sequence: 0,
            previous_entry_hash: None,
            operation,
            created_at,
        };
        Self::sign_body(body, signer)
    }

    pub fn append<F>(
        &self,
        operation: DeviceWriterOperation,
        created_at: u64,
        signer: F,
    ) -> Result<Self, DeviceWriterLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let body = DeviceWriterLogEntryBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            identity: self.body.identity.clone(),
            device_id: self.body.device_id.clone(),
            device_sequence: self.body.device_sequence + 1,
            previous_entry_hash: Some(self.entry_hash()),
            operation,
            created_at,
        };
        Self::sign_body(body, signer)
    }

    pub fn entry_hash(&self) -> DeviceWriterLogEntryHash {
        DeviceWriterLogEntryHash(*blake3::hash(&self.body.canonical_bytes()).as_bytes())
    }

    fn sign_body<F>(body: DeviceWriterLogEntryBody, signer: F) -> Result<Self, DeviceWriterLogError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        validate_body(&body)?;
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }
}

impl DeviceWriterLogEntryBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.record_type);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        put_string(&mut bytes, &self.identity.to_string());
        put_string(&mut bytes, &self.device_id);
        bytes.extend_from_slice(&self.device_sequence.to_be_bytes());
        match &self.previous_entry_hash {
            Some(hash) => {
                bytes.push(1);
                bytes.extend_from_slice(&hash.0);
            }
            None => bytes.push(0),
        }
        self.operation.encode(&mut bytes);
        bytes.extend_from_slice(&self.created_at.to_be_bytes());
        bytes
    }
}

pub fn merge_device_writer_logs<I>(
    authority: &VerifiedIdentityAuthority,
    logs: I,
) -> Result<MergedDeviceIdentityState, DeviceWriterLogError>
where
    I: IntoIterator<Item = Vec<DeviceWriterLogEntry>>,
{
    let mut singleton_candidates: BTreeMap<String, Vec<MergedDeviceWriterEntry>> = BTreeMap::new();
    let mut append_records: BTreeMap<String, Vec<MergedDeviceWriterEntry>> = BTreeMap::new();
    let mut rejected_entries = Vec::new();

    for log in logs {
        let Some(first) = log.first() else {
            continue;
        };
        verify_device_log_shape(&authority.identity, &log)?;
        let device = authority.devices.get(&first.body.device_id);

        for entry in &log {
            let entry_hash = entry.entry_hash();
            let Some(device) = device else {
                rejected_entries.push(rejected_entry(
                    entry,
                    entry_hash,
                    DeviceWriterRejectionReason::UnknownDevice,
                ));
                continue;
            };
            verify_signature(
                &device.signing_public_key,
                &entry.body.canonical_bytes(),
                &entry.signature,
            )?;
            if device.status == AuthorizedDeviceStatus::Revoked
                && !authority.device_can_write(&entry.body.device_id, entry.body.device_sequence)
            {
                rejected_entries.push(rejected_entry(
                    entry,
                    entry_hash,
                    DeviceWriterRejectionReason::RevokedDevice,
                ));
                continue;
            }

            match &entry.body.operation {
                DeviceWriterOperation::SetPath {
                    path,
                    content_id,
                    mode,
                } => {
                    let merged = MergedDeviceWriterEntry {
                        identity: entry.body.identity.clone(),
                        device_id: entry.body.device_id.clone(),
                        device_sequence: entry.body.device_sequence,
                        entry_hash,
                        created_at: entry.body.created_at,
                        content_id: content_id.clone(),
                    };
                    match mode {
                        DeviceWriterPathMode::Singleton => {
                            singleton_candidates
                                .entry(path.clone())
                                .or_default()
                                .push(merged);
                        }
                        DeviceWriterPathMode::Append => {
                            append_records.entry(path.clone()).or_default().push(merged);
                        }
                    }
                }
            }
        }
    }

    for records in append_records.values_mut() {
        records.sort_by(entry_order);
    }

    let mut singleton_paths = BTreeMap::new();
    let mut singleton_conflicts = BTreeMap::new();
    for (path, mut candidates) in singleton_candidates {
        candidates.sort_by(entry_order);
        let winner = candidates
            .pop()
            .expect("singleton candidates map only stores non-empty values");
        if !candidates.is_empty() {
            singleton_conflicts.insert(path.clone(), candidates);
        }
        singleton_paths.insert(path, winner);
    }

    rejected_entries.sort_by(|left, right| {
        (
            left.identity.to_string(),
            &left.device_id,
            left.device_sequence,
            &left.entry_hash.0,
        )
            .cmp(&(
                right.identity.to_string(),
                &right.device_id,
                right.device_sequence,
                &right.entry_hash.0,
            ))
    });

    Ok(MergedDeviceIdentityState {
        identity: authority.identity.clone(),
        singleton_paths,
        singleton_conflicts,
        append_records,
        rejected_entries,
    })
}

pub fn resolve_merged_device_jolt_address(
    address: &JoltAddress,
    merged: &MergedDeviceIdentityState,
) -> Result<ResolvedJoltTarget, DeviceWriterLogError> {
    if address.identity() != &merged.identity {
        return Err(DeviceWriterLogError::ResolveIdentityMismatch);
    }
    let path = address.path().to_string();
    let entry = merged
        .singleton_paths
        .get(address.path())
        .ok_or_else(|| DeviceWriterLogError::PathNotFound { path: path.clone() })?;

    Ok(ResolvedJoltTarget {
        identity: merged.identity.clone(),
        path,
        content_id: entry.content_id.clone(),
        reachability: Vec::new(),
    })
}

fn verify_device_log_shape(
    identity: &IdentityId,
    log: &[DeviceWriterLogEntry],
) -> Result<(), DeviceWriterLogError> {
    let Some(first) = log.first() else {
        return Ok(());
    };
    for (index, entry) in log.iter().enumerate() {
        validate_body(&entry.body)?;
        if &entry.body.identity != identity {
            return Err(DeviceWriterLogError::IdentityMismatch);
        }
        if entry.body.device_id != first.body.device_id {
            return Err(DeviceWriterLogError::DeviceChanged { index });
        }
        if index == 0 {
            if entry.body.device_sequence != 0 {
                return Err(DeviceWriterLogError::InvalidGenesisSequence);
            }
            if entry.body.previous_entry_hash.is_some() {
                return Err(DeviceWriterLogError::GenesisHasPreviousHash);
            }
            continue;
        }

        let expected = log[index - 1].body.device_sequence + 1;
        if entry.body.device_sequence != expected {
            return Err(DeviceWriterLogError::OutOfOrderSequence {
                index,
                expected,
                actual: entry.body.device_sequence,
            });
        }
        let expected_previous_hash = log[index - 1].entry_hash();
        if entry.body.previous_entry_hash.as_ref() != Some(&expected_previous_hash) {
            return Err(DeviceWriterLogError::BrokenPreviousHash { index });
        }
    }
    Ok(())
}

fn rejected_entry(
    entry: &DeviceWriterLogEntry,
    entry_hash: DeviceWriterLogEntryHash,
    reason: DeviceWriterRejectionReason,
) -> RejectedDeviceWriterEntry {
    let content_id = match &entry.body.operation {
        DeviceWriterOperation::SetPath { content_id, .. } => content_id.clone(),
    };
    RejectedDeviceWriterEntry {
        identity: entry.body.identity.clone(),
        device_id: entry.body.device_id.clone(),
        device_sequence: entry.body.device_sequence,
        entry_hash,
        content_id,
        reason,
    }
}

fn validate_body(body: &DeviceWriterLogEntryBody) -> Result<(), DeviceWriterLogError> {
    if body.record_type != RECORD_TYPE {
        return Err(DeviceWriterLogError::UnsupportedEntryType);
    }
    if body.version != RECORD_VERSION {
        return Err(DeviceWriterLogError::UnsupportedEntryVersion);
    }
    validate_device_id(&body.device_id)?;
    match &body.operation {
        DeviceWriterOperation::SetPath { path, .. } => validate_path(&body.identity, path),
    }
}

fn validate_device_id(device_id: &str) -> Result<(), DeviceWriterLogError> {
    if device_id.trim().is_empty() {
        Err(DeviceWriterLogError::EmptyDeviceId)
    } else {
        Ok(())
    }
}

fn validate_path(identity: &IdentityId, path: &str) -> Result<(), DeviceWriterLogError> {
    if path.trim().is_empty() {
        return Err(DeviceWriterLogError::EmptyPath);
    }
    if !path.starts_with('/') || JoltAddress::new(identity.clone(), path).is_err() {
        Err(DeviceWriterLogError::InvalidPath)
    } else {
        Ok(())
    }
}

fn verify_signature(
    public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), DeviceWriterLogError> {
    if public_key.len() != PUBLIC_KEY_LEN {
        return Err(DeviceWriterLogError::InvalidSignature);
    }
    validate_signature_len(signature)?;

    let public_key: [u8; PUBLIC_KEY_LEN] = public_key
        .try_into()
        .map_err(|_| DeviceWriterLogError::InvalidSignature)?;
    let signature: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| DeviceWriterLogError::InvalidSignatureLength)?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| DeviceWriterLogError::InvalidSignature)?;
    let signature = Signature::from_bytes(&signature);
    verifying_key
        .verify_strict(payload, &signature)
        .map_err(|_| DeviceWriterLogError::InvalidSignature)
}

fn validate_signature_len(signature: &[u8]) -> Result<(), DeviceWriterLogError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(DeviceWriterLogError::InvalidSignatureLength)
    }
}

fn entry_order(
    left: &MergedDeviceWriterEntry,
    right: &MergedDeviceWriterEntry,
) -> std::cmp::Ordering {
    (
        left.created_at,
        left.device_sequence,
        &left.device_id,
        &left.entry_hash.0,
    )
        .cmp(&(
            right.created_at,
            right.device_sequence,
            &right.device_id,
            &right.entry_hash.0,
        ))
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u64).to_be_bytes());
    out.extend_from_slice(value);
}

fn put_string(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}
