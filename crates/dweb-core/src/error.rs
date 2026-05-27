use thiserror::Error;

#[derive(Error, Debug)]
pub enum DwebError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Hash error: {0}")]
    Hash(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Content not found: {0}")]
    ContentNotFound(String),

    #[error("Invalid content ID: {0}")]
    InvalidContentId(String),

    #[error("Invalid identity address: {0}")]
    InvalidIdentityAddress(String),
}
