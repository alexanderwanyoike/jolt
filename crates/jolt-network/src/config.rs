use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct RelayPinPolicy {
    #[serde(default)]
    pub allowed_identities: BTreeSet<String>,
}

impl RelayPinPolicy {
    pub fn is_allowed(&self, identity: &str) -> bool {
        self.allowed_identities.contains(identity)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeRelayCapability {
    Unknown,
    DiscoveryOnly,
    Pinning,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct HomeRelayConfig {
    pub peer_id: String,
    pub multiaddr: String,
    pub capability: HomeRelayCapability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

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
    /// Optional override for how many DHT keys this node may advertise itself
    /// as a provider for. Production uses libp2p's default when unset; load
    /// harnesses may raise it explicitly without changing normal nodes.
    pub provider_record_capacity: Option<usize>,
    /// Bootstrap relays saved in persistent node config.
    pub configured_bootstrap_relays: Vec<String>,
    /// Bootstrap relays used for this daemon start after merging config, CLI,
    /// and optional built-in defaults.
    pub effective_bootstrap_relays: Vec<String>,
    /// Whether this node is intentionally acting as a bootstrap/discovery relay.
    pub bootstrap_relay: bool,
    /// User-selected home relay for delegated availability.
    pub home_relay: Option<HomeRelayConfig>,
    /// Operator allowlist for accepting owner-signed relay pin requests.
    pub relay_pin_policy: RelayPinPolicy,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            enable_mdns: true,
            enable_upnp: true,
            p2p_port: 0,
            provider_record_capacity: None,
            configured_bootstrap_relays: Vec::new(),
            effective_bootstrap_relays: Vec::new(),
            bootstrap_relay: false,
            home_relay: None,
            relay_pin_policy: RelayPinPolicy::default(),
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
            provider_record_capacity: None,
            configured_bootstrap_relays: Vec::new(),
            effective_bootstrap_relays: Vec::new(),
            bootstrap_relay: false,
            home_relay: None,
            relay_pin_policy: RelayPinPolicy::default(),
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
        assert!(config.provider_record_capacity.is_none());
        assert!(config.effective_bootstrap_relays.is_empty());
        assert!(!config.bootstrap_relay);
        assert!(config.home_relay.is_none());
        assert!(config.relay_pin_policy.allowed_identities.is_empty());
    }

    #[test]
    fn relay_pin_policy_defaults_to_denying_every_identity() {
        assert!(!RelayPinPolicy::default().is_allowed("alice"));
    }
}
