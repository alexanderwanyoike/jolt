use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryFailureCode {
    NoBootstrapRelays,
    RelayUnreachable,
    RelayMeshEmpty,
    IdentityProviderNotFound,
    IdentityHeadInvalid,
    ContentProviderNotFound,
    ContentFetchFailed,
    ContentHashMismatch,
}

impl DiscoveryFailureCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoBootstrapRelays => "no_bootstrap_relays",
            Self::RelayUnreachable => "relay_unreachable",
            Self::RelayMeshEmpty => "relay_mesh_empty",
            Self::IdentityProviderNotFound => "identity_provider_not_found",
            Self::IdentityHeadInvalid => "identity_head_invalid",
            Self::ContentProviderNotFound => "content_provider_not_found",
            Self::ContentFetchFailed => "content_fetch_failed",
            Self::ContentHashMismatch => "content_hash_mismatch",
        }
    }
}

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error("Swarm error: {0}")]
    Swarm(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

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

    #[error("{message}")]
    DiscoveryFailed {
        code: DiscoveryFailureCode,
        message: String,
    },
}
