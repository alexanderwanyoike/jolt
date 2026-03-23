use libp2p::{Multiaddr, PeerId};

use crate::error::NetworkError;

/// Parse a bootstrap multiaddr that includes a /p2p/<peer_id> suffix.
/// Returns the PeerId and the transport-only multiaddr (without /p2p/ suffix).
pub fn parse_bootstrap_addr(addr: &str) -> Result<(PeerId, Multiaddr), NetworkError> {
    let multiaddr: Multiaddr = addr
        .parse()
        .map_err(|e| NetworkError::Swarm(format!("Invalid bootstrap multiaddr: {e}")))?;

    // Extract PeerId from the /p2p/ component
    let peer_id = multiaddr
        .iter()
        .find_map(|p| {
            if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
                Some(peer_id)
            } else {
                None
            }
        })
        .ok_or(NetworkError::NoBootstrapPeers)?;

    // Remove the /p2p/ component to get the transport address
    let transport_addr: Multiaddr = multiaddr
        .iter()
        .filter(|p| !matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
        .collect();

    Ok((peer_id, transport_addr))
}

/// Default bootstrap peer addresses.
/// These are well-known public dweb nodes that help new nodes join the DHT.
pub fn default_bootstrap_peers() -> Vec<String> {
    // Will be populated when bootstrap nodes are deployed
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_multiaddr_extracts_peer_id() {
        let addr = "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
        let (peer_id, transport) = parse_bootstrap_addr(addr).unwrap();
        assert_eq!(
            peer_id.to_string(),
            "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        );
        assert!(!transport.to_string().contains("p2p"));
    }

    #[test]
    fn parse_bootstrap_multiaddr_fails_without_peer_id() {
        let addr = "/ip4/89.167.68.65/tcp/4001";
        let result = parse_bootstrap_addr(addr);
        assert!(result.is_err());
    }

    #[test]
    fn default_bootstrap_list_exists() {
        // Currently empty, but the function exists
        let _peers = default_bootstrap_peers();
    }
}
