use std::path::PathBuf;

use jolt_identity::{
    decrypt_identity_export, encrypt_identity_export, identity_from_export,
    ExportedIdentityEncryptionKeypair, IdentityExportBundle, IdentityExportError,
    IdentityExportSource, NodeIdentity,
};
use jolt_store::{CacheConfig, ContentStore, LocalIdentityEncryptionKeypair};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone)]
pub struct IdentityRecoveryStore {
    identity_dir: PathBuf,
    content_store_dir: PathBuf,
    jolt_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportIdentityRequest {
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportIdentityResponse {
    pub identity: String,
    pub encryption_key_count: usize,
    pub bundle: IdentityExportBundle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportIdentityRequest {
    #[serde(default)]
    pub passphrase: Option<String>,
    pub bundle: IdentityExportBundle,
    #[serde(default)]
    pub allow_overwrite: bool,
    #[serde(default)]
    pub as_local_identity: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportIdentityResponse {
    pub identity: String,
    pub imported: bool,
    pub restart_required: bool,
    pub encryption_key_count: usize,
    pub app_sessions_imported: bool,
}

pub struct LocalIdentityImport {
    pub identity: NodeIdentity,
    pub label: Option<String>,
    pub encryption_keypairs: Vec<ExportedIdentityEncryptionKeypair>,
    pub encryption_key_count: usize,
}

#[derive(Debug, Error)]
pub enum IdentityRecoveryError {
    #[error("identity export/import failed: {0}")]
    Bundle(#[from] IdentityExportError),
    #[error("identity storage error: {0}")]
    Identity(#[from] jolt_identity::IdentityError),
    #[error("content store error: {0}")]
    Store(#[from] jolt_store::StoreError),
    #[error("import would overwrite existing identity {existing}; pass allow_overwrite to replace it with {incoming}")]
    WouldOverwrite { existing: String, incoming: String },
}

impl IdentityRecoveryStore {
    pub fn open_default() -> Self {
        let base = directories::ProjectDirs::from("net", "jolt", "jolt")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".jolt")
            });
        Self::new(
            base.join("identity"),
            base.join("data"),
            Some(env!("CARGO_PKG_VERSION").to_string()),
        )
    }

    pub fn new(
        identity_dir: PathBuf,
        content_store_dir: PathBuf,
        jolt_version: Option<String>,
    ) -> Self {
        Self {
            identity_dir,
            content_store_dir,
            jolt_version,
        }
    }

    pub fn export_identity(
        &self,
        request: ExportIdentityRequest,
    ) -> Result<ExportIdentityResponse, IdentityRecoveryError> {
        let identity = NodeIdentity::load(&self.identity_dir)?;
        let encryption_keys = self.load_exported_encryption_keypairs()?;
        self.export_node_identity(identity, encryption_keys, request)
    }

    pub fn export_local_identity(
        &self,
        identity: NodeIdentity,
        encryption_keys: Vec<ExportedIdentityEncryptionKeypair>,
        request: ExportIdentityRequest,
    ) -> Result<ExportIdentityResponse, IdentityRecoveryError> {
        self.export_node_identity(identity, encryption_keys, request)
    }

    pub fn local_identities_dir(&self) -> PathBuf {
        self.identity_dir
            .parent()
            .unwrap_or(&self.identity_dir)
            .join("local-identities")
    }

    fn export_node_identity(
        &self,
        identity: NodeIdentity,
        encryption_keys: Vec<ExportedIdentityEncryptionKeypair>,
        request: ExportIdentityRequest,
    ) -> Result<ExportIdentityResponse, IdentityRecoveryError> {
        let encryption_key_count = encryption_keys.len();
        let identity_address = identity.jolt_address().to_string();
        let bundle = encrypt_identity_export(
            &identity,
            encryption_keys,
            request.passphrase.as_deref().unwrap_or(""),
            IdentityExportSource {
                jolt_version: self.jolt_version.clone(),
                exported_at: unix_now(),
                label: request.label,
            },
        )?;
        Ok(ExportIdentityResponse {
            identity: identity_address,
            encryption_key_count,
            bundle,
        })
    }

    pub fn import_identity(
        &self,
        request: ImportIdentityRequest,
    ) -> Result<ImportIdentityResponse, IdentityRecoveryError> {
        let plaintext =
            decrypt_identity_export(&request.bundle, request.passphrase.as_deref().unwrap_or(""))?;
        let identity = identity_from_export(&plaintext)?;
        let incoming = identity.jolt_address().to_string();
        if let Ok(existing) = NodeIdentity::load(&self.identity_dir) {
            let existing_address = existing.jolt_address().to_string();
            if existing_address != incoming && !request.allow_overwrite {
                return Err(IdentityRecoveryError::WouldOverwrite {
                    existing: existing_address,
                    incoming,
                });
            }
        }

        identity.save(&self.identity_dir)?;
        self.save_imported_encryption_keypairs(&plaintext.identity_encryption_keys)?;
        Ok(ImportIdentityResponse {
            identity: identity.jolt_address().to_string(),
            imported: true,
            restart_required: true,
            encryption_key_count: plaintext.identity_encryption_keys.len(),
            app_sessions_imported: false,
        })
    }

    pub fn import_local_identity(
        &self,
        request: ImportIdentityRequest,
    ) -> Result<LocalIdentityImport, IdentityRecoveryError> {
        let plaintext =
            decrypt_identity_export(&request.bundle, request.passphrase.as_deref().unwrap_or(""))?;
        let identity = identity_from_export(&plaintext)?;
        Ok(LocalIdentityImport {
            identity,
            label: plaintext.source.label,
            encryption_keypairs: plaintext.identity_encryption_keys.clone(),
            encryption_key_count: plaintext.identity_encryption_keys.len(),
        })
    }

    fn load_exported_encryption_keypairs(
        &self,
    ) -> Result<Vec<ExportedIdentityEncryptionKeypair>, IdentityRecoveryError> {
        let store = ContentStore::open(&self.content_store_dir, CacheConfig::default())?;
        Ok(store
            .load_local_identity_encryption_keypair()?
            .into_iter()
            .map(|keypair| ExportedIdentityEncryptionKeypair {
                public_key: keypair.public_key,
                private_key: keypair.private_key,
            })
            .collect())
    }

    fn save_imported_encryption_keypairs(
        &self,
        keypairs: &[ExportedIdentityEncryptionKeypair],
    ) -> Result<(), IdentityRecoveryError> {
        if let Some(keypair) = keypairs.first() {
            let store = ContentStore::open(&self.content_store_dir, CacheConfig::default())?;
            store.save_local_identity_encryption_keypair(&LocalIdentityEncryptionKeypair {
                public_key: keypair.public_key.clone(),
                private_key: keypair.private_key.clone(),
            })?;
        }
        Ok(())
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
