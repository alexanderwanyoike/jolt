use thiserror::Error;

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Key not found at: {0}")]
    KeyNotFound(String),

    #[error("Invalid key: {0}")]
    InvalidKey(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Signing error: {0}")]
    SigningError(String),

    #[error("Verification error: {0}")]
    VerificationError(String),
}
