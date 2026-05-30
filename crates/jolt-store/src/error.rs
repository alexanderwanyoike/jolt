use thiserror::Error;

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Cache full: not enough space after evicting non-pinned content")]
    CacheFull,

    #[error("Content not found: {0}")]
    ContentNotFound(String),

    #[error("Content already exists: {0}")]
    AlreadyExists(String),
}
