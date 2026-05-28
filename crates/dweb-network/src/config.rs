use libp2p::Multiaddr;

/// Configuration for the network node.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Bootstrap peer multiaddrs (must include /p2p/<peer_id> suffix).
    pub bootstrap_peers: Vec<Multiaddr>,
    /// Enable mDNS for LAN discovery.
    pub enable_mdns: bool,
    /// Enable UPnP for automatic port mapping.
    pub enable_upnp: bool,
    /// Fixed UDP port for iroh P2P (0 = random). Use a fixed port on servers
    /// so the firewall can be configured once.
    pub p2p_port: u16,
    /// Bootstrap relays saved in persistent node config.
    pub configured_bootstrap_relays: Vec<String>,
    /// Bootstrap relays used for this daemon start after merging config, CLI,
    /// and optional built-in defaults.
    pub effective_bootstrap_relays: Vec<String>,
    /// Whether this node is intentionally acting as a bootstrap/discovery relay.
    pub bootstrap_relay: bool,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            enable_mdns: true,
            enable_upnp: true,
            p2p_port: 0,
            configured_bootstrap_relays: Vec::new(),
            effective_bootstrap_relays: Vec::new(),
            bootstrap_relay: false,
        }
    }
}

impl NetworkConfig {
    /// Config for tests: no bootstrap, mDNS enabled, no UPnP.
    pub fn test_config() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            enable_mdns: true,
            enable_upnp: false,
            p2p_port: 0,
            configured_bootstrap_relays: Vec::new(),
            effective_bootstrap_relays: Vec::new(),
            bootstrap_relay: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_network_config_has_no_public_bootstrap_relays() {
        let config = NetworkConfig::default();

        assert!(config.bootstrap_peers.is_empty());
        assert!(config.configured_bootstrap_relays.is_empty());
        assert!(config.effective_bootstrap_relays.is_empty());
        assert!(!config.bootstrap_relay);
    }
}
