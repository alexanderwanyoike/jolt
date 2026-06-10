use libp2p::{Multiaddr, PeerId};

use crate::error::NetworkError;

pub const DEFAULT_BOOTSTRAP_RELAY: &str =
    "/ip4/167.233.106.111/udp/4001/quic-v1/p2p/12D3KooWDmwLRmG4pZa7GcUM1P3CXM9TwMjtoM69QqTrwXD63tqi";

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

/// Generate the QUIC variant of a TCP multiaddr.
/// `/ip4/x.x.x.x/tcp/PORT` -> `/ip4/x.x.x.x/udp/PORT/quic-v1`
/// Returns None if the addr is not TCP or is already QUIC.
pub fn quic_variant(addr: &Multiaddr) -> Option<Multiaddr> {
    let addr_str = addr.to_string();
    if addr_str.contains("quic-v1") {
        return None; // already QUIC
    }
    if !addr_str.contains("/tcp/") {
        return None; // not TCP, can't convert
    }

    // Find the TCP port and build a QUIC addr
    let components: Vec<libp2p::multiaddr::Protocol> = addr.iter().collect();
    let mut quic_components = Vec::new();

    for component in &components {
        match component {
            libp2p::multiaddr::Protocol::Tcp(port) => {
                quic_components.push(libp2p::multiaddr::Protocol::Udp(*port));
                quic_components.push(libp2p::multiaddr::Protocol::QuicV1);
            }
            other => {
                quic_components.push(other.clone());
            }
        }
    }

    let quic_addr: Multiaddr = quic_components.into_iter().collect();
    Some(quic_addr)
}

/// Parse a bootstrap addr and return both the original and its QUIC variant.
/// This ensures nodes connect via both TCP and QUIC for better address discovery.
pub fn parse_bootstrap_addr_with_quic(
    addr: &str,
) -> Result<Vec<(PeerId, Multiaddr)>, NetworkError> {
    let (peer_id, transport) = parse_bootstrap_addr(addr)?;
    let mut results = vec![(peer_id, transport.clone())];

    if let Some(quic_addr) = quic_variant(&transport) {
        results.push((peer_id, quic_addr));
    }

    Ok(results)
}

/// Optional built-in bootstrap peer addresses.
///
/// These relays are rendezvous/bootstrap contacts only. They help fresh nodes
/// enter the network but do not own identities, content, or pinning authority.
pub fn default_bootstrap_peers() -> Vec<String> {
    vec![DEFAULT_BOOTSTRAP_RELAY.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootstrap_multiaddr_extracts_peer_id() {
        let addr =
            "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
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
        assert_eq!(
            default_bootstrap_peers(),
            vec![DEFAULT_BOOTSTRAP_RELAY.to_string()]
        );

        for addr in default_bootstrap_peers() {
            parse_bootstrap_addr(&addr).expect("default bootstrap peer must be valid");
        }
    }

    #[test]
    fn quic_variant_from_tcp() {
        let tcp: Multiaddr = "/ip4/89.167.68.65/tcp/4001".parse().unwrap();
        let quic = quic_variant(&tcp).unwrap();
        assert_eq!(quic.to_string(), "/ip4/89.167.68.65/udp/4001/quic-v1");
    }

    #[test]
    fn quic_variant_already_quic_returns_none() {
        let quic: Multiaddr = "/ip4/89.167.68.65/udp/4001/quic-v1".parse().unwrap();
        assert!(quic_variant(&quic).is_none());
    }

    #[test]
    fn parse_bootstrap_with_quic_returns_both() {
        let addr =
            "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
        let results = parse_bootstrap_addr_with_quic(addr).unwrap();
        assert_eq!(results.len(), 2);
        // First is original TCP
        assert!(results[0].1.to_string().contains("/tcp/"));
        // Second is QUIC variant
        assert!(results[1].1.to_string().contains("/udp/4001/quic-v1"));
    }

    #[test]
    fn parse_bootstrap_quic_input_returns_one() {
        let addr = "/ip4/89.167.68.65/udp/4001/quic-v1/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
        let results = parse_bootstrap_addr_with_quic(addr).unwrap();
        // QUIC input has no TCP variant to generate, just returns the original
        assert_eq!(results.len(), 1);
    }
}
