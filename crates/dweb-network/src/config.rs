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
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            enable_mdns: true,
            enable_upnp: true,
            p2p_port: 0,
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
        }
    }
}
