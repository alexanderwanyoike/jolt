use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use data_encoding::BASE64URL_NOPAD;
use jolt_core::{IdentityEncryptionKey, IdentityEncryptionPrivateKey, IdentityId};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use thiserror::Error;

use crate::NodeIdentity;

const MAGIC: &str = "jolt.identity.export";
const PLAINTEXT_TYPE: &str = "jolt.identity.export.plaintext";
const VERSION: u16 = 1;
const KDF_NAME: &str = "argon2id";
const CIPHER_NAME: &str = "xchacha20poly1305";
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const MAX_KDF_MEMORY_KIB: u32 = 65_536;
const MAX_KDF_ITERATIONS: u32 = 3;
const MAX_KDF_PARALLELISM: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportBundle {
    pub magic: String,
    pub version: u16,
    pub kdf: IdentityExportKdf,
    pub cipher: IdentityExportCipher,
    pub created_at: u64,
    pub identity: String,
    pub ciphertext: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportKdf {
    pub name: String,
    pub params: IdentityExportKdfParams,
    pub salt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportKdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportCipher {
    pub name: String,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportPlaintext {
    pub r#type: String,
    pub version: u16,
    pub identity: ExportedIdentity,
    pub identity_encryption_keys: Vec<ExportedIdentityEncryptionKeypair>,
    pub source: IdentityExportSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedIdentity {
    pub id: String,
    pub address: String,
    pub public_key_ed25519: String,
    pub secret_key_ed25519: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedIdentityEncryptionKeypair {
    pub public_key: IdentityEncryptionKey,
    pub private_key: IdentityEncryptionPrivateKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityExportSource {
    pub jolt_version: Option<String>,
    pub exported_at: u64,
    pub label: Option<String>,
}

#[derive(Debug, Error)]
pub enum IdentityExportError {
    #[error("identity export bundle has unsupported magic or version")]
    UnsupportedBundle,
    #[error("identity export bundle uses unsupported encryption parameters")]
    UnsupportedProtection,
    #[error("identity export bundle encoding is invalid: {0}")]
    InvalidEncoding(String),
    #[error("identity export passphrase is incorrect or bundle was tampered with")]
    DecryptionFailed,
    #[error("identity export plaintext is invalid: {0}")]
    InvalidPlaintext(String),
    #[error("identity export key derivation failed")]
    KdfFailed,
}

pub fn encrypt_identity_export(
    identity: &NodeIdentity,
    encryption_keys: Vec<ExportedIdentityEncryptionKeypair>,
    passphrase: &str,
    source: IdentityExportSource,
) -> Result<IdentityExportBundle, IdentityExportError> {
    let created_at = source.exported_at;
    let address = identity.jolt_address().to_string();
    let plaintext = IdentityExportPlaintext {
        r#type: PLAINTEXT_TYPE.to_string(),
        version: VERSION,
        identity: ExportedIdentity {
            id: identity.identity_id().to_string(),
            address: address.clone(),
            public_key_ed25519: encode(identity.public_key_bytes()),
            secret_key_ed25519: encode(identity.signing_key_bytes()),
        },
        identity_encryption_keys: encryption_keys,
        source,
    };
    validate_plaintext(&plaintext, &address)?;

    let kdf_params = IdentityExportKdfParams {
        memory_kib: 65_536,
        iterations: 3,
        parallelism: 1,
    };
    let mut salt = [0_u8; SALT_LEN];
    let mut nonce = [0_u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let key = derive_key(passphrase, &salt, &kdf_params)?;
    let aad = associated_data(&address);
    let plaintext_bytes = serde_json::to_vec(&plaintext)
        .map_err(|e| IdentityExportError::InvalidEncoding(e.to_string()))?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &plaintext_bytes,
                aad: &aad,
            },
        )
        .map_err(|_| IdentityExportError::DecryptionFailed)?;

    Ok(IdentityExportBundle {
        magic: MAGIC.to_string(),
        version: VERSION,
        kdf: IdentityExportKdf {
            name: KDF_NAME.to_string(),
            params: kdf_params,
            salt: encode(salt),
        },
        cipher: IdentityExportCipher {
            name: CIPHER_NAME.to_string(),
            nonce: encode(nonce),
        },
        created_at,
        identity: address,
        ciphertext: encode(ciphertext),
    })
}

pub fn decrypt_identity_export(
    bundle: &IdentityExportBundle,
    passphrase: &str,
) -> Result<IdentityExportPlaintext, IdentityExportError> {
    if bundle.magic != MAGIC || bundle.version != VERSION {
        return Err(IdentityExportError::UnsupportedBundle);
    }
    if bundle.kdf.name != KDF_NAME || bundle.cipher.name != CIPHER_NAME {
        return Err(IdentityExportError::UnsupportedProtection);
    }
    if bundle.kdf.params.memory_kib > MAX_KDF_MEMORY_KIB
        || bundle.kdf.params.iterations > MAX_KDF_ITERATIONS
        || bundle.kdf.params.parallelism > MAX_KDF_PARALLELISM
    {
        return Err(IdentityExportError::UnsupportedProtection);
    }
    let salt = decode(&bundle.kdf.salt)?;
    let nonce = decode(&bundle.cipher.nonce)?;
    if salt.len() != SALT_LEN || nonce.len() != NONCE_LEN {
        return Err(IdentityExportError::UnsupportedProtection);
    }
    let ciphertext = decode(&bundle.ciphertext)?;
    let key = derive_key(passphrase, &salt, &bundle.kdf.params)?;
    let aad = associated_data(&bundle.identity);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext_bytes = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| IdentityExportError::DecryptionFailed)?;
    let plaintext: IdentityExportPlaintext = serde_json::from_slice(&plaintext_bytes)
        .map_err(|e| IdentityExportError::InvalidPlaintext(e.to_string()))?;
    validate_plaintext(&plaintext, &bundle.identity)?;
    Ok(plaintext)
}

pub fn write_identity_export_file(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)
}

pub fn identity_from_export(
    plaintext: &IdentityExportPlaintext,
) -> Result<NodeIdentity, IdentityExportError> {
    let secret = decode(&plaintext.identity.secret_key_ed25519)?;
    let identity = NodeIdentity::from_signing_key_bytes(&secret)
        .map_err(|e| IdentityExportError::InvalidPlaintext(e.to_string()))?;
    validate_plaintext(plaintext, &identity.jolt_address().to_string())?;
    Ok(identity)
}

fn validate_plaintext(
    plaintext: &IdentityExportPlaintext,
    expected_address: &str,
) -> Result<(), IdentityExportError> {
    if plaintext.r#type != PLAINTEXT_TYPE || plaintext.version != VERSION {
        return Err(IdentityExportError::InvalidPlaintext(
            "unsupported plaintext type or version".to_string(),
        ));
    }
    let secret = decode(&plaintext.identity.secret_key_ed25519)?;
    let identity = NodeIdentity::from_signing_key_bytes(&secret)
        .map_err(|e| IdentityExportError::InvalidPlaintext(e.to_string()))?;
    let public_key = decode(&plaintext.identity.public_key_ed25519)?;
    if public_key != identity.public_key_bytes() {
        return Err(IdentityExportError::InvalidPlaintext(
            "public key does not match signing secret".to_string(),
        ));
    }
    let expected_id = IdentityId::from_public_key(identity.public_key_bytes());
    if plaintext.identity.id != expected_id.to_string()
        || plaintext.identity.address != identity.jolt_address().to_string()
        || plaintext.identity.address != expected_address
    {
        return Err(IdentityExportError::InvalidPlaintext(
            "identity id/address mismatch".to_string(),
        ));
    }
    for keypair in &plaintext.identity_encryption_keys {
        if keypair.public_key.key_id != keypair.private_key.key_id {
            return Err(IdentityExportError::InvalidPlaintext(
                "encryption key id mismatch".to_string(),
            ));
        }
        if keypair.private_key.identity != expected_id {
            return Err(IdentityExportError::InvalidPlaintext(
                "encryption private key identity mismatch".to_string(),
            ));
        }
        if keypair.public_key.suite_family != keypair.private_key.suite_family {
            return Err(IdentityExportError::InvalidPlaintext(
                "encryption key suite mismatch".to_string(),
            ));
        }
        if keypair.public_key.public_key.len() != 32 || keypair.private_key.private_key.len() != 32
        {
            return Err(IdentityExportError::InvalidPlaintext(
                "encryption key material has invalid length".to_string(),
            ));
        }
    }
    Ok(())
}

fn derive_key(
    passphrase: &str,
    salt: &[u8],
    params: &IdentityExportKdfParams,
) -> Result<[u8; KEY_LEN], IdentityExportError> {
    let params = Params::new(
        params.memory_kib,
        params.iterations,
        params.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|_| IdentityExportError::UnsupportedProtection)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0_u8; KEY_LEN];
    argon2
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|_| IdentityExportError::KdfFailed)?;
    Ok(key)
}

fn associated_data(identity: &str) -> Vec<u8> {
    format!("{MAGIC}\0{VERSION}\0{identity}\0{KDF_NAME}\0{CIPHER_NAME}").into_bytes()
}

fn encode(bytes: impl AsRef<[u8]>) -> String {
    BASE64URL_NOPAD.encode(bytes.as_ref())
}

fn decode(value: &str) -> Result<Vec<u8>, IdentityExportError> {
    BASE64URL_NOPAD
        .decode(value.as_bytes())
        .map_err(|e| IdentityExportError::InvalidEncoding(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolt_core::generate_identity_encryption_keypair;

    #[test]
    fn encrypted_identity_export_round_trips_key_material() {
        let identity = NodeIdentity::generate();
        let keypair = exported_keypair(&identity);
        let source = source();

        let bundle = encrypt_identity_export(
            &identity,
            vec![keypair.clone()],
            "correct horse battery staple",
            source,
        )
        .unwrap();
        assert_eq!(bundle.magic, MAGIC);
        assert_eq!(bundle.identity, identity.jolt_address().to_string());
        assert_ne!(bundle.ciphertext, "");

        let plaintext = decrypt_identity_export(&bundle, "correct horse battery staple").unwrap();
        let imported = identity_from_export(&plaintext).unwrap();
        assert_eq!(imported.public_key_bytes(), identity.public_key_bytes());
        assert_eq!(plaintext.identity_encryption_keys, vec![keypair]);
    }

    #[test]
    fn encrypted_identity_export_rejects_wrong_passphrase() {
        let identity = NodeIdentity::generate();
        let bundle = encrypt_identity_export(
            &identity,
            Vec::new(),
            "correct horse battery staple",
            source(),
        )
        .unwrap();

        let err = decrypt_identity_export(&bundle, "incorrect passphrase").unwrap_err();
        assert!(matches!(err, IdentityExportError::DecryptionFailed));
    }

    #[test]
    fn encrypted_identity_export_rejects_identity_header_mismatch() {
        let identity = NodeIdentity::generate();
        let mut bundle = encrypt_identity_export(
            &identity,
            Vec::new(),
            "correct horse battery staple",
            source(),
        )
        .unwrap();
        bundle.identity = NodeIdentity::generate().jolt_address().to_string();

        let err = decrypt_identity_export(&bundle, "correct horse battery staple").unwrap_err();
        assert!(matches!(err, IdentityExportError::DecryptionFailed));
    }

    #[test]
    fn encrypted_identity_export_allows_empty_passphrase() {
        let identity = NodeIdentity::generate();
        let bundle = encrypt_identity_export(&identity, Vec::new(), "", source()).unwrap();
        let plaintext = decrypt_identity_export(&bundle, "").unwrap();

        let imported = identity_from_export(&plaintext).unwrap();
        assert_eq!(imported.public_key_bytes(), identity.public_key_bytes());
    }

    #[test]
    fn encrypted_identity_export_rejects_excessive_kdf_cost_before_derivation() {
        let identity = NodeIdentity::generate();
        let mut bundle =
            encrypt_identity_export(&identity, Vec::new(), "passphrase", source()).unwrap();
        bundle.kdf.params.memory_kib = 65_537;

        let err = decrypt_identity_export(&bundle, "passphrase").unwrap_err();
        assert!(matches!(err, IdentityExportError::UnsupportedProtection));
    }

    #[cfg(unix)]
    #[test]
    fn identity_export_files_are_owner_only_even_when_overwriting() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity.jolt-identity");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_identity_export_file(&path, b"recovery bundle").unwrap();

        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fn exported_keypair(identity: &NodeIdentity) -> ExportedIdentityEncryptionKeypair {
        let (public_key, private_key) = generate_identity_encryption_keypair(
            identity.identity_id(),
            "enc_x25519_local_v0".to_string(),
            1_780_579_200,
        );
        ExportedIdentityEncryptionKeypair {
            public_key,
            private_key,
        }
    }

    fn source() -> IdentityExportSource {
        IdentityExportSource {
            jolt_version: Some("test".to_string()),
            exported_at: 1_780_579_200,
            label: Some("test export".to_string()),
        }
    }
}
