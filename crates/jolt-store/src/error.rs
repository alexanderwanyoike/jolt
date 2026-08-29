use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Cache full: not enough space after evicting non-pinned content")]
    CacheFull,

    #[error("Remote identity state is larger than the configured cache capacity")]
    RemoteIdentityStateTooLarge,

    #[error("Content not found: {0}")]
    ContentNotFound(String),

    #[error("Content bytes do not match content id: {0}")]
    ContentMismatch(String),

    #[error("Content already exists: {0}")]
    AlreadyExists(String),

    #[error("Invalid relay record: {0}")]
    InvalidRelayRecord(String),
}
