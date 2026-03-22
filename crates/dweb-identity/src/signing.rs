use ed25519_dalek::{Signature, VerifyingKey};

use crate::error::IdentityError;

/// Verify an Ed25519 signature against a public key and data.
/// Returns Ok(true) if valid, Ok(false) if invalid signature,
/// or Err if the key/signature bytes are malformed.
pub fn verify_signature(
    public_key_bytes: &[u8],
    data: &[u8],
    signature_bytes: &[u8],
) -> Result<bool, IdentityError> {
    let key_array: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| IdentityError::InvalidKey("public key must be 32 bytes".to_string()))?;

    let verifying_key = VerifyingKey::from_bytes(&key_array)
        .map_err(|e| IdentityError::InvalidKey(e.to_string()))?;

    let sig_array: [u8; 64] = signature_bytes
        .try_into()
        .map_err(|_| IdentityError::VerificationError("signature must be 64 bytes".to_string()))?;

    let signature = Signature::from_bytes(&sig_array);

    use ed25519_dalek::Verifier;
    match verifying_key.verify(data, &signature) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_with_invalid_key_length_returns_error() {
        let result = verify_signature(&[0u8; 16], b"data", &[0u8; 64]);
        assert!(result.is_err());
    }

    #[test]
    fn verify_with_invalid_signature_length_returns_error() {
        let result = verify_signature(&[0u8; 32], b"data", &[0u8; 32]);
        assert!(result.is_err());
    }
}
