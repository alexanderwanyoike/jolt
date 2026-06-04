use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng as AeadOsRng, Payload},
    ChaCha20Poly1305,
};
use ed25519_dalek::{Signature, VerifyingKey};
use hpke::{
    aead::ChaCha20Poly1305 as HpkeChaCha20Poly1305, kdf::HkdfSha256, kem::X25519HkdfSha256,
    single_shot_open, single_shot_seal, Deserializable, Kem as KemTrait, OpModeR, OpModeS,
    Serializable,
};
use rand_core::{OsRng as HpkeOsRng, UnwrapErr};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{IdentityEncryptionKey, IdentityId};

pub const ENCRYPTED_OBJECT_SUITE_ID: &str =
    "jolt.enc.v1.x25519-hkdf-sha256-chacha20poly1305.ed25519";

const RECORD_TYPE: &str = "jolt.encrypted_object";
const RECORD_VERSION: u16 = 1;
const CONTENT_ALG: &str = "CHACHA20-POLY1305";
const WRAP_ALG: &str = "HPKE-BASE-X25519-HKDF-SHA256-CHACHA20POLY1305";
const KEY_SUITE_FAMILY: &str = "x25519-hkdf-sha256";
const KEY_TYPE: &str = "OKP";
const KEY_CURVE: &str = "X25519";
const KEY_STATUS_ACTIVE: &str = "active";
const DOMAIN_SEPARATOR: &[u8] = b"jolt:encrypted-object-envelope:v1\0";
const CONTENT_AAD_DOMAIN: &[u8] = b"jolt:encrypted-object-content-aad:v1\0";
const WRAP_AAD_DOMAIN: &[u8] = b"jolt:recipient-content-key-wrap-aad:v1\0";
const HPKE_INFO: &[u8] = b"jolt:hpke-content-key-wrap:v1";
const PUBLIC_KEY_LEN: usize = 32;
const PRIVATE_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;
const CONTENT_NONCE_LEN: usize = 12;

type HpkeKem = X25519HkdfSha256;
type HpkeKdf = HkdfSha256;
type HpkeAead = HpkeChaCha20Poly1305;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EncryptedObjectError {
    #[error("author public key must be 32 bytes")]
    InvalidAuthorPublicKey,

    #[error("author identity does not match author public key")]
    AuthorIdentityMismatch,

    #[error("signature must be 64 bytes")]
    InvalidSignatureLength,

    #[error("invalid signature")]
    InvalidSignature,

    #[error("encrypted object suite is not supported")]
    UnsupportedSuite,

    #[error("encrypted object record type is not supported")]
    UnsupportedRecordType,

    #[error("encrypted object version is not supported")]
    UnsupportedVersion,

    #[error("recipient encryption key is not supported")]
    UnsupportedRecipientKey,

    #[error("recipient private key does not match a recipient wrap")]
    RecipientNotFound,

    #[error("recipient private key must be 32 bytes")]
    InvalidRecipientPrivateKey,

    #[error("encrypted object decryption failed")]
    DecryptionFailed,

    #[error("encrypted object encryption failed")]
    EncryptionFailed,

    #[error("encrypted object content nonce is invalid")]
    InvalidContentNonce,

    #[error("encrypted object encoding is invalid")]
    InvalidEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEncryptionPrivateKey {
    pub identity: IdentityId,
    pub key_id: String,
    pub suite_family: String,
    pub private_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectRecipient {
    pub identity: IdentityId,
    pub key: IdentityEncryptionKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectAuthor {
    pub identity: IdentityId,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectPlaintext {
    pub media_type: String,
    pub schema: Option<String>,
    pub declared_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectContentEncryption {
    pub alg: String,
    pub nonce: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectRecipientWrap {
    pub recipient_identity: IdentityId,
    pub recipient_key_id: String,
    pub wrap_alg: String,
    pub encapped_key: Vec<u8>,
    pub wrapped_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectBody {
    pub record_type: String,
    pub version: u16,
    pub suite_id: String,
    pub author: EncryptedObjectAuthor,
    pub plaintext: EncryptedObjectPlaintext,
    pub content_encryption: EncryptedObjectContentEncryption,
    pub ciphertext: Vec<u8>,
    pub recipients: Vec<EncryptedObjectRecipientWrap>,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedObjectEnvelope {
    pub body: EncryptedObjectBody,
    pub signature: Vec<u8>,
}

pub fn generate_identity_encryption_keypair(
    identity: IdentityId,
    key_id: String,
    created_at: u64,
) -> (IdentityEncryptionKey, IdentityEncryptionPrivateKey) {
    let mut rng = UnwrapErr(HpkeOsRng);
    let (private_key, public_key) = HpkeKem::gen_keypair(&mut rng);
    let public_key = public_key.to_bytes().to_vec();
    let private_key = private_key.to_bytes().to_vec();

    (
        IdentityEncryptionKey {
            key_id: key_id.clone(),
            suite_family: KEY_SUITE_FAMILY.to_string(),
            key_type: KEY_TYPE.to_string(),
            curve: KEY_CURVE.to_string(),
            public_key,
            created_at,
            not_before: created_at,
            expires_at: None,
            status: KEY_STATUS_ACTIVE.to_string(),
        },
        IdentityEncryptionPrivateKey {
            identity,
            key_id,
            suite_family: KEY_SUITE_FAMILY.to_string(),
            private_key,
        },
    )
}

impl EncryptedObjectEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn encrypt<F>(
        author_public_key: impl Into<Vec<u8>>,
        author_identity: IdentityId,
        plaintext: &[u8],
        media_type: String,
        schema: Option<String>,
        recipients: Vec<EncryptedObjectRecipient>,
        created_at: u64,
        signer: F,
    ) -> Result<Self, EncryptedObjectError>
    where
        F: FnOnce(&[u8]) -> Vec<u8>,
    {
        let author_public_key = author_public_key.into();
        validate_author_identity(&author_public_key, &author_identity)?;
        let content_key = ChaCha20Poly1305::generate_key(&mut AeadOsRng);
        let nonce = ChaCha20Poly1305::generate_nonce(&mut AeadOsRng);
        let plaintext_meta = EncryptedObjectPlaintext {
            media_type,
            schema,
            declared_size: plaintext.len() as u64,
        };
        let content_encryption = EncryptedObjectContentEncryption {
            alg: CONTENT_ALG.to_string(),
            nonce: nonce.to_vec(),
        };
        let content_aad = content_aad(
            &author_identity,
            &plaintext_meta,
            &content_encryption,
            created_at,
        );
        let cipher = ChaCha20Poly1305::new(&content_key);
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: &content_aad,
                },
            )
            .map_err(|_| EncryptedObjectError::EncryptionFailed)?;

        let mut recipient_wraps = Vec::with_capacity(recipients.len());
        for recipient in recipients {
            validate_recipient_key(&recipient.key)?;
            let public_key =
                <HpkeKem as KemTrait>::PublicKey::from_bytes(&recipient.key.public_key)
                    .map_err(|_| EncryptedObjectError::UnsupportedRecipientKey)?;
            let wrap_aad = wrap_aad(&author_identity, &recipient.identity, &recipient.key.key_id);
            let mut rng = UnwrapErr(HpkeOsRng);
            let (encapped_key, wrapped_key) = single_shot_seal::<HpkeAead, HpkeKdf, HpkeKem, _>(
                &OpModeS::Base,
                &public_key,
                HPKE_INFO,
                content_key.as_slice(),
                &wrap_aad,
                &mut rng,
            )
            .map_err(|_| EncryptedObjectError::EncryptionFailed)?;
            recipient_wraps.push(EncryptedObjectRecipientWrap {
                recipient_identity: recipient.identity,
                recipient_key_id: recipient.key.key_id,
                wrap_alg: WRAP_ALG.to_string(),
                encapped_key: encapped_key.to_bytes().to_vec(),
                wrapped_key,
            });
        }

        let body = EncryptedObjectBody {
            record_type: RECORD_TYPE.to_string(),
            version: RECORD_VERSION,
            suite_id: ENCRYPTED_OBJECT_SUITE_ID.to_string(),
            author: EncryptedObjectAuthor {
                identity: author_identity,
                public_key: author_public_key,
            },
            plaintext: plaintext_meta,
            content_encryption,
            ciphertext,
            recipients: recipient_wraps,
            created_at,
        };
        let signature = signer(&body.canonical_bytes());
        validate_signature_len(&signature)?;
        Ok(Self { body, signature })
    }

    pub fn verify_signature(&self) -> Result<(), EncryptedObjectError> {
        validate_body(&self.body)?;
        verify_signature(
            &self.body.author.public_key,
            &self.body.canonical_bytes(),
            &self.signature,
        )
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, EncryptedObjectError> {
        serde_json::to_vec(self).map_err(|_| EncryptedObjectError::InvalidEncoding)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EncryptedObjectError> {
        let envelope: Self =
            serde_json::from_slice(bytes).map_err(|_| EncryptedObjectError::InvalidEncoding)?;
        validate_body(&envelope.body)?;
        validate_signature_len(&envelope.signature)?;
        Ok(envelope)
    }
}

impl EncryptedObjectBody {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(DOMAIN_SEPARATOR);
        put_string(&mut bytes, &self.record_type);
        bytes.extend_from_slice(&self.version.to_be_bytes());
        put_string(&mut bytes, &self.suite_id);
        put_string(&mut bytes, &self.author.identity.to_string());
        put_bytes(&mut bytes, &self.author.public_key);
        put_string(&mut bytes, &self.plaintext.media_type);
        match &self.plaintext.schema {
            Some(schema) => {
                bytes.push(1);
                put_string(&mut bytes, schema);
            }
            None => bytes.push(0),
        }
        bytes.extend_from_slice(&self.plaintext.declared_size.to_be_bytes());
        put_string(&mut bytes, &self.content_encryption.alg);
        put_bytes(&mut bytes, &self.content_encryption.nonce);
        put_bytes(&mut bytes, &self.ciphertext);
        bytes.extend_from_slice(&(self.recipients.len() as u64).to_be_bytes());
        for recipient in &self.recipients {
            put_string(&mut bytes, &recipient.recipient_identity.to_string());
            put_string(&mut bytes, &recipient.recipient_key_id);
            put_string(&mut bytes, &recipient.wrap_alg);
            put_bytes(&mut bytes, &recipient.encapped_key);
            put_bytes(&mut bytes, &recipient.wrapped_key);
        }
        bytes.extend_from_slice(&self.created_at.to_be_bytes());
        bytes
    }
}

pub fn decrypt_encrypted_object_for_recipient(
    envelope: &EncryptedObjectEnvelope,
    recipient_key: &IdentityEncryptionPrivateKey,
) -> Result<Vec<u8>, EncryptedObjectError> {
    envelope.verify_signature()?;
    validate_private_key(recipient_key)?;
    let recipient = envelope
        .body
        .recipients
        .iter()
        .find(|recipient| {
            recipient.recipient_identity == recipient_key.identity
                && recipient.recipient_key_id == recipient_key.key_id
        })
        .ok_or(EncryptedObjectError::RecipientNotFound)?;

    let private_key = <HpkeKem as KemTrait>::PrivateKey::from_bytes(&recipient_key.private_key)
        .map_err(|_| EncryptedObjectError::InvalidRecipientPrivateKey)?;
    let encapped_key = <HpkeKem as KemTrait>::EncappedKey::from_bytes(&recipient.encapped_key)
        .map_err(|_| EncryptedObjectError::DecryptionFailed)?;
    let wrap_aad = wrap_aad(
        &envelope.body.author.identity,
        &recipient.recipient_identity,
        &recipient.recipient_key_id,
    );
    let content_key = single_shot_open::<HpkeAead, HpkeKdf, HpkeKem>(
        &OpModeR::Base,
        &private_key,
        &encapped_key,
        HPKE_INFO,
        &recipient.wrapped_key,
        &wrap_aad,
    )
    .map_err(|_| EncryptedObjectError::DecryptionFailed)?;
    if content_key.len() != 32 {
        return Err(EncryptedObjectError::DecryptionFailed);
    }

    let cipher = ChaCha20Poly1305::new_from_slice(&content_key)
        .map_err(|_| EncryptedObjectError::DecryptionFailed)?;
    let content_aad = content_aad(
        &envelope.body.author.identity,
        &envelope.body.plaintext,
        &envelope.body.content_encryption,
        envelope.body.created_at,
    );
    cipher
        .decrypt(
            envelope.body.content_encryption.nonce.as_slice().into(),
            Payload {
                msg: &envelope.body.ciphertext,
                aad: &content_aad,
            },
        )
        .map_err(|_| EncryptedObjectError::DecryptionFailed)
}

fn validate_body(body: &EncryptedObjectBody) -> Result<(), EncryptedObjectError> {
    if body.record_type != RECORD_TYPE {
        return Err(EncryptedObjectError::UnsupportedRecordType);
    }
    if body.version != RECORD_VERSION {
        return Err(EncryptedObjectError::UnsupportedVersion);
    }
    if body.suite_id != ENCRYPTED_OBJECT_SUITE_ID {
        return Err(EncryptedObjectError::UnsupportedSuite);
    }
    if body.content_encryption.alg != CONTENT_ALG {
        return Err(EncryptedObjectError::UnsupportedSuite);
    }
    if body.content_encryption.nonce.len() != CONTENT_NONCE_LEN {
        return Err(EncryptedObjectError::InvalidContentNonce);
    }
    validate_author_identity(&body.author.public_key, &body.author.identity)
}

fn validate_author_identity(
    author_public_key: &[u8],
    author_identity: &IdentityId,
) -> Result<(), EncryptedObjectError> {
    if author_public_key.len() != PUBLIC_KEY_LEN {
        return Err(EncryptedObjectError::InvalidAuthorPublicKey);
    }
    let public_key: [u8; PUBLIC_KEY_LEN] = author_public_key
        .try_into()
        .map_err(|_| EncryptedObjectError::InvalidAuthorPublicKey)?;
    if IdentityId::from_public_key(public_key) != *author_identity {
        return Err(EncryptedObjectError::AuthorIdentityMismatch);
    }
    Ok(())
}

fn validate_recipient_key(key: &IdentityEncryptionKey) -> Result<(), EncryptedObjectError> {
    if key.suite_family == KEY_SUITE_FAMILY
        && key.key_type == KEY_TYPE
        && key.curve == KEY_CURVE
        && key.public_key.len() == PUBLIC_KEY_LEN
        && key.status == KEY_STATUS_ACTIVE
    {
        Ok(())
    } else {
        Err(EncryptedObjectError::UnsupportedRecipientKey)
    }
}

fn validate_private_key(key: &IdentityEncryptionPrivateKey) -> Result<(), EncryptedObjectError> {
    if key.suite_family != KEY_SUITE_FAMILY {
        return Err(EncryptedObjectError::UnsupportedRecipientKey);
    }
    if key.private_key.len() != PRIVATE_KEY_LEN {
        return Err(EncryptedObjectError::InvalidRecipientPrivateKey);
    }
    Ok(())
}

fn verify_signature(
    public_key: &[u8],
    payload: &[u8],
    signature: &[u8],
) -> Result<(), EncryptedObjectError> {
    if public_key.len() != PUBLIC_KEY_LEN {
        return Err(EncryptedObjectError::InvalidAuthorPublicKey);
    }
    validate_signature_len(signature)?;

    let key_array: [u8; PUBLIC_KEY_LEN] = public_key
        .try_into()
        .map_err(|_| EncryptedObjectError::InvalidAuthorPublicKey)?;
    let signature_array: [u8; SIGNATURE_LEN] = signature
        .try_into()
        .map_err(|_| EncryptedObjectError::InvalidSignatureLength)?;
    let key = VerifyingKey::from_bytes(&key_array)
        .map_err(|_| EncryptedObjectError::InvalidAuthorPublicKey)?;
    let signature = Signature::from_bytes(&signature_array);
    key.verify_strict(payload, &signature)
        .map_err(|_| EncryptedObjectError::InvalidSignature)
}

fn validate_signature_len(signature: &[u8]) -> Result<(), EncryptedObjectError> {
    if signature.len() == SIGNATURE_LEN {
        Ok(())
    } else {
        Err(EncryptedObjectError::InvalidSignatureLength)
    }
}

fn content_aad(
    author_identity: &IdentityId,
    plaintext: &EncryptedObjectPlaintext,
    content_encryption: &EncryptedObjectContentEncryption,
    created_at: u64,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(CONTENT_AAD_DOMAIN);
    put_string(&mut bytes, ENCRYPTED_OBJECT_SUITE_ID);
    put_string(&mut bytes, &author_identity.to_string());
    put_string(&mut bytes, &plaintext.media_type);
    match &plaintext.schema {
        Some(schema) => {
            bytes.push(1);
            put_string(&mut bytes, schema);
        }
        None => bytes.push(0),
    }
    bytes.extend_from_slice(&plaintext.declared_size.to_be_bytes());
    put_string(&mut bytes, &content_encryption.alg);
    bytes.extend_from_slice(&created_at.to_be_bytes());
    bytes
}

fn wrap_aad(
    author_identity: &IdentityId,
    recipient_identity: &IdentityId,
    recipient_key_id: &str,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(WRAP_AAD_DOMAIN);
    put_string(&mut bytes, ENCRYPTED_OBJECT_SUITE_ID);
    put_string(&mut bytes, &author_identity.to_string());
    put_string(&mut bytes, &recipient_identity.to_string());
    put_string(&mut bytes, recipient_key_id);
    put_string(&mut bytes, WRAP_ALG);
    bytes
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
        decrypt_encrypted_object_for_recipient, generate_identity_encryption_keypair, ContentId,
        EncryptedObjectEnvelope, EncryptedObjectError, EncryptedObjectRecipient, IdentityId,
    };

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn author_identity(author: &SigningKey) -> IdentityId {
        IdentityId::from_public_key(author.verifying_key().to_bytes())
    }

    fn recipient_identity(seed: u8) -> IdentityId {
        IdentityId::from_public_key(signing_key(seed).verifying_key().to_bytes())
    }

    #[test]
    fn recipient_can_decrypt_owner_signed_encrypted_object() {
        let author = signing_key(7);
        let author_public_key = author.verifying_key().to_bytes();
        let author_identity = IdentityId::from_public_key(author_public_key);
        let recipient_identity =
            IdentityId::from_public_key(signing_key(8).verifying_key().to_bytes());
        let (recipient_public_key, recipient_private_key) = generate_identity_encryption_keypair(
            recipient_identity.clone(),
            "enc_x25519_test".to_string(),
            100,
        );

        let envelope = EncryptedObjectEnvelope::encrypt(
            author_public_key,
            author_identity.clone(),
            b"private bytes",
            "text/plain".to_string(),
            None,
            vec![EncryptedObjectRecipient {
                identity: recipient_identity,
                key: recipient_public_key,
            }],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        let plaintext =
            decrypt_encrypted_object_for_recipient(&envelope, &recipient_private_key).unwrap();

        assert_eq!(plaintext, b"private bytes");
        assert_eq!(envelope.body.author.identity, author_identity);
        assert_eq!(envelope.body.recipients.len(), 1);
        assert_ne!(envelope.body.ciphertext, b"private bytes");
    }

    #[test]
    fn multiple_recipients_decrypt_the_same_encrypted_object_ciphertext() {
        let author = signing_key(7);
        let author_identity = author_identity(&author);
        let bob_identity = recipient_identity(8);
        let clara_identity = recipient_identity(9);
        let (bob_public_key, bob_private_key) = generate_identity_encryption_keypair(
            bob_identity.clone(),
            "bob_x25519".to_string(),
            100,
        );
        let (clara_public_key, clara_private_key) = generate_identity_encryption_keypair(
            clara_identity.clone(),
            "clara_x25519".to_string(),
            100,
        );

        let envelope = EncryptedObjectEnvelope::encrypt(
            author.verifying_key().to_bytes(),
            author_identity,
            b"shared private bytes",
            "text/plain".to_string(),
            None,
            vec![
                EncryptedObjectRecipient {
                    identity: bob_identity,
                    key: bob_public_key,
                },
                EncryptedObjectRecipient {
                    identity: clara_identity,
                    key: clara_public_key,
                },
            ],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        let bob_plaintext =
            decrypt_encrypted_object_for_recipient(&envelope, &bob_private_key).unwrap();
        let clara_plaintext =
            decrypt_encrypted_object_for_recipient(&envelope, &clara_private_key).unwrap();

        assert_eq!(bob_plaintext, b"shared private bytes");
        assert_eq!(clara_plaintext, b"shared private bytes");
        assert_eq!(envelope.body.recipients.len(), 2);
        assert_ne!(
            envelope.body.recipients[0].wrapped_key,
            envelope.body.recipients[1].wrapped_key
        );
    }

    #[test]
    fn non_recipient_cannot_decrypt_encrypted_object() {
        let author = signing_key(7);
        let author_identity = author_identity(&author);
        let bob_identity = recipient_identity(8);
        let mallory_identity = recipient_identity(9);
        let (bob_public_key, _bob_private_key) = generate_identity_encryption_keypair(
            bob_identity.clone(),
            "bob_x25519".to_string(),
            100,
        );
        let (_mallory_public_key, mallory_private_key) = generate_identity_encryption_keypair(
            mallory_identity,
            "mallory_x25519".to_string(),
            100,
        );

        let envelope = EncryptedObjectEnvelope::encrypt(
            author.verifying_key().to_bytes(),
            author_identity,
            b"private bytes",
            "text/plain".to_string(),
            None,
            vec![EncryptedObjectRecipient {
                identity: bob_identity,
                key: bob_public_key,
            }],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        assert_eq!(
            decrypt_encrypted_object_for_recipient(&envelope, &mallory_private_key),
            Err(EncryptedObjectError::RecipientNotFound)
        );
    }

    #[test]
    fn tampered_encrypted_object_fails_before_decryption() {
        let author = signing_key(7);
        let author_identity = author_identity(&author);
        let bob_identity = recipient_identity(8);
        let (bob_public_key, bob_private_key) = generate_identity_encryption_keypair(
            bob_identity.clone(),
            "bob_x25519".to_string(),
            100,
        );
        let mut envelope = EncryptedObjectEnvelope::encrypt(
            author.verifying_key().to_bytes(),
            author_identity,
            b"private bytes",
            "text/plain".to_string(),
            None,
            vec![EncryptedObjectRecipient {
                identity: bob_identity,
                key: bob_public_key,
            }],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();
        envelope.body.ciphertext[0] ^= 1;

        assert_eq!(
            decrypt_encrypted_object_for_recipient(&envelope, &bob_private_key),
            Err(EncryptedObjectError::InvalidSignature)
        );
    }

    #[test]
    fn ciphertext_integrity_failure_rejects_decryption_even_with_valid_signature() {
        let author = signing_key(7);
        let author_identity = author_identity(&author);
        let bob_identity = recipient_identity(8);
        let (bob_public_key, bob_private_key) = generate_identity_encryption_keypair(
            bob_identity.clone(),
            "bob_x25519".to_string(),
            100,
        );
        let mut envelope = EncryptedObjectEnvelope::encrypt(
            author.verifying_key().to_bytes(),
            author_identity,
            b"private bytes",
            "text/plain".to_string(),
            None,
            vec![EncryptedObjectRecipient {
                identity: bob_identity,
                key: bob_public_key,
            }],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();
        envelope.body.ciphertext[0] ^= 1;
        envelope.signature = author
            .sign(&envelope.body.canonical_bytes())
            .to_bytes()
            .to_vec();

        assert_eq!(
            decrypt_encrypted_object_for_recipient(&envelope, &bob_private_key),
            Err(EncryptedObjectError::DecryptionFailed)
        );
    }

    #[test]
    fn encrypted_object_round_trips_as_content_addressed_bytes() {
        let author = signing_key(7);
        let author_identity = author_identity(&author);
        let bob_identity = recipient_identity(8);
        let (bob_public_key, bob_private_key) = generate_identity_encryption_keypair(
            bob_identity.clone(),
            "bob_x25519".to_string(),
            100,
        );
        let envelope = EncryptedObjectEnvelope::encrypt(
            author.verifying_key().to_bytes(),
            author_identity,
            b"persisted private bytes",
            "application/octet-stream".to_string(),
            None,
            vec![EncryptedObjectRecipient {
                identity: bob_identity,
                key: bob_public_key,
            }],
            120,
            |bytes| author.sign(bytes).to_bytes().to_vec(),
        )
        .unwrap();

        let bytes = envelope.to_bytes().unwrap();
        let content_id = ContentId::from_bytes(&bytes);
        let decoded = EncryptedObjectEnvelope::from_bytes(&bytes).unwrap();
        let plaintext = decrypt_encrypted_object_for_recipient(&decoded, &bob_private_key).unwrap();

        assert!(content_id.verify(&bytes));
        assert_eq!(plaintext, b"persisted private bytes");
    }
}
