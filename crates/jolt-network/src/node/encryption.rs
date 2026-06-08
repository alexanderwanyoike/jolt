use jolt_core::{
    decrypt_encrypted_object_for_recipient, generate_identity_encryption_keypair,
    EncryptedObjectEnvelope, EncryptedObjectRecipient, IdentityEncryptionKey,
    IdentityEncryptionKeyRecord, IdentityEncryptionPrivateKey, IDENTITY_ENCRYPTION_KEYS_PATH,
};
use jolt_identity::NodeIdentity;
use jolt_store::{ContentStore, LocalIdentityEncryptionKeypair};

use crate::command::{DecryptedObjectResponse, EncryptedObjectResponse};
use crate::error::NetworkError;

use super::{unix_now, NetworkNode};

impl NetworkNode {
    pub(super) fn ensure_local_identity_encryption_key(
        &mut self,
    ) -> Result<IdentityEncryptionKey, NetworkError> {
        if self.local_encryption_key.is_none() {
            let now = unix_now();
            let (public_key, private_key) = generate_identity_encryption_keypair(
                self.identity.identity_id(),
                "enc_x25519_local_v0".to_string(),
                now,
            );
            self.store
                .save_local_identity_encryption_keypair(&LocalIdentityEncryptionKeypair {
                    public_key: public_key.clone(),
                    private_key: private_key.clone(),
                })
                .map_err(|e| NetworkError::Protocol(e.to_string()))?;
            self.local_encryption_key = Some((public_key, private_key));
        }

        let public_key = self
            .local_encryption_key
            .as_ref()
            .map(|(public_key, _)| public_key.clone())
            .expect("local encryption key is initialized");
        if !self.local_encryption_key_published {
            let now = unix_now();
            let record = IdentityEncryptionKeyRecord::new(
                self.identity.public_key_bytes(),
                self.identity.identity_id(),
                vec![public_key.clone()],
                now,
                now,
                |bytes| self.identity.sign(bytes),
            )
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
            let record_bytes =
                serde_json::to_vec(&record).map_err(|e| NetworkError::Protocol(e.to_string()))?;
            self.publish_bytes_at_path(&record_bytes, IDENTITY_ENCRYPTION_KEYS_PATH)?;
            self.local_encryption_key_published = true;
        }

        Ok(public_key)
    }

    pub(super) fn load_persisted_local_encryption_key(
        store: &ContentStore,
        identity: &NodeIdentity,
    ) -> Result<Option<(IdentityEncryptionKey, IdentityEncryptionPrivateKey)>, NetworkError> {
        let Some(keypair) = store
            .load_local_identity_encryption_keypair()
            .map_err(|e| NetworkError::Protocol(e.to_string()))?
        else {
            return Ok(None);
        };

        if keypair.public_key.key_id != keypair.private_key.key_id {
            return Err(NetworkError::Protocol(
                "local identity encryption keypair has mismatched key ids".to_string(),
            ));
        }
        if keypair.private_key.identity != identity.identity_id() {
            return Err(NetworkError::Protocol(
                "local identity encryption private key belongs to a different identity".to_string(),
            ));
        }

        Ok(Some((keypair.public_key, keypair.private_key)))
    }

    pub(super) fn encrypt_object(
        &self,
        plaintext: Vec<u8>,
        content_type: String,
        recipients: Vec<EncryptedObjectRecipient>,
    ) -> Result<EncryptedObjectResponse, NetworkError> {
        let envelope = EncryptedObjectEnvelope::encrypt(
            self.identity.public_key_bytes(),
            self.identity.identity_id(),
            &plaintext,
            content_type,
            None,
            recipients,
            unix_now(),
            |bytes| self.identity.sign(bytes),
        )
        .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        let recipient_count = envelope.body.recipients.len();
        let data = envelope
            .to_bytes()
            .map_err(|e| NetworkError::Protocol(e.to_string()))?;
        Ok(EncryptedObjectResponse {
            size: data.len() as u64,
            data,
            recipient_count,
        })
    }

    pub(super) fn decrypt_object(
        &self,
        encrypted_object: &[u8],
    ) -> Result<DecryptedObjectResponse, NetworkError> {
        let Some((_, private_key)) = self.local_encryption_key.as_ref() else {
            return Err(NetworkError::Protocol(
                "local identity encryption key is not available".to_string(),
            ));
        };
        let envelope = EncryptedObjectEnvelope::from_bytes(encrypted_object)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        let content_type = envelope.body.plaintext.media_type.clone();
        let plaintext = decrypt_encrypted_object_for_recipient(&envelope, private_key)
            .map_err(|e| NetworkError::InvalidInput(e.to_string()))?;
        Ok(DecryptedObjectResponse {
            size: plaintext.len() as u64,
            plaintext,
            content_type,
        })
    }
}
