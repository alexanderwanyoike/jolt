use std::path::Path;

use ed25519_dalek::SigningKey;
use rand::rngs::OsRng;

use crate::error::IdentityError;

pub struct NodeIdentity {
    signing_key: SigningKey,
}

impl NodeIdentity {
    /// Generate a new random Ed25519 keypair.
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    /// Save the keypair to a directory.
    /// Private key is stored as raw bytes with restrictive permissions.
    /// TODO: M6 - encrypt at rest with Argon2id KDF + ChaCha20-Poly1305
    pub fn save(&self, dir: &Path) -> Result<(), IdentityError> {
        std::fs::create_dir_all(dir)?;

        let key_path = dir.join("keypair.bin");
        let pub_path = dir.join("public_key.pub");

        std::fs::write(&key_path, self.signing_key.to_bytes())?;
        std::fs::write(&pub_path, self.signing_key.verifying_key().to_bytes())?;

        // Set restrictive permissions on private key (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Load a keypair from a directory.
    pub fn load(dir: &Path) -> Result<Self, IdentityError> {
        let key_path = dir.join("keypair.bin");
        if !key_path.exists() {
            return Err(IdentityError::KeyNotFound(
                key_path.display().to_string(),
            ));
        }

        let key_bytes = std::fs::read(&key_path)?;
        let key_array: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| IdentityError::InvalidKey("key must be 32 bytes".to_string()))?;

        let signing_key = SigningKey::from_bytes(&key_array);
        Ok(Self { signing_key })
    }

    /// Load an existing keypair, or generate and save a new one.
    pub fn load_or_generate(dir: &Path) -> Result<Self, IdentityError> {
        match Self::load(dir) {
            Ok(identity) => Ok(identity),
            Err(IdentityError::KeyNotFound(_)) => {
                let identity = Self::generate();
                identity.save(dir)?;
                Ok(identity)
            }
            Err(e) => Err(e),
        }
    }

    /// Get the raw public key bytes (32 bytes).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Get the Ed25519 verifying (public) key.
    pub fn verifying_key(&self) -> ed25519_dalek::VerifyingKey {
        self.signing_key.verifying_key()
    }

    /// Sign data with this identity's private key.
    pub fn sign(&self, data: &[u8]) -> Vec<u8> {
        use ed25519_dalek::Signer;
        let signature = self.signing_key.sign(data);
        signature.to_bytes().to_vec()
    }

    /// Convert to a libp2p identity keypair.
    pub fn to_libp2p_keypair(&self) -> libp2p::identity::Keypair {
        let secret_bytes = self.signing_key.to_bytes();
        let mut full_bytes = secret_bytes.to_vec();
        full_bytes.extend_from_slice(&self.signing_key.verifying_key().to_bytes());
        let ed25519_keypair =
            libp2p::identity::ed25519::Keypair::try_from_bytes(&mut full_bytes)
                .expect("valid ed25519 key");
        libp2p::identity::Keypair::from(ed25519_keypair)
    }

    /// Get the libp2p PeerId derived from this identity.
    pub fn peer_id(&self) -> libp2p::PeerId {
        self.to_libp2p_keypair().public().to_peer_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn generate_produces_32_byte_public_key() {
        let identity = NodeIdentity::generate();
        assert_eq!(identity.public_key_bytes().len(), 32);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::generate();
        let original_pubkey = identity.public_key_bytes();

        identity.save(dir.path()).unwrap();
        let loaded = NodeIdentity::load(dir.path()).unwrap();

        assert_eq!(original_pubkey, loaded.public_key_bytes());
    }

    #[test]
    fn load_nonexistent_returns_key_not_found() {
        let dir = tempdir().unwrap();
        let result = NodeIdentity::load(dir.path());
        assert!(matches!(result, Err(IdentityError::KeyNotFound(_))));
    }

    #[test]
    fn load_or_generate_creates_new_if_missing() {
        let dir = tempdir().unwrap();
        let identity = NodeIdentity::load_or_generate(dir.path()).unwrap();
        let pubkey = identity.public_key_bytes();

        // Loading again should return the same key
        let loaded = NodeIdentity::load_or_generate(dir.path()).unwrap();
        assert_eq!(pubkey, loaded.public_key_bytes());
    }

    #[test]
    fn to_libp2p_keypair_produces_valid_peer_id() {
        let identity = NodeIdentity::generate();
        let peer_id = identity.peer_id();
        // PeerId should be non-empty and consistent
        let peer_id2 = identity.peer_id();
        assert_eq!(peer_id, peer_id2);
    }

    #[test]
    fn sign_produces_verifiable_signature() {
        let identity = NodeIdentity::generate();
        let data = b"test data to sign";
        let signature = identity.sign(data);

        let result =
            crate::signing::verify_signature(&identity.public_key_bytes(), data, &signature);
        assert!(result.unwrap());
    }

    #[test]
    fn sign_fails_verification_with_wrong_data() {
        let identity = NodeIdentity::generate();
        let signature = identity.sign(b"original data");

        let result =
            crate::signing::verify_signature(&identity.public_key_bytes(), b"wrong data", &signature);
        assert!(!result.unwrap());
    }

    #[test]
    fn sign_fails_verification_with_wrong_key() {
        let identity1 = NodeIdentity::generate();
        let identity2 = NodeIdentity::generate();
        let data = b"test data";
        let signature = identity1.sign(data);

        let result =
            crate::signing::verify_signature(&identity2.public_key_bytes(), data, &signature);
        assert!(!result.unwrap());
    }
}
