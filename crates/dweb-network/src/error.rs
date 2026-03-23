use thiserror::Error;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Swarm error: {0}")]
    Swarm(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Content not found: {0}")]
    ContentNotFound(String),

    #[error("Verification failed: hash mismatch")]
    VerificationFailed,

    #[error("Timeout waiting for response")]
    Timeout,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No peers available")]
    NoPeers,

    #[error("DHT error: {0}")]
    Dht(String),

    #[error("Bootstrap failed: no known peers")]
    NoBootstrapPeers,

    #[error("Provider not found for content: {0}")]
    ProviderNotFound(String),
}
