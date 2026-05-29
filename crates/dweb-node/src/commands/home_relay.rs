use anyhow::Result;
use dweb_network::bootstrap::parse_bootstrap_addr;
use dweb_network::{HomeRelayCapability, HomeRelayConfig};

use crate::cli::HomeRelayCapabilityArg;
use crate::client::DaemonClient;
use crate::config::NodeConfig;
use crate::daemon;

#[derive(Debug, Eq, PartialEq)]
pub enum HomeRelayChange {
    Cleared,
    NotConfigured,
}

pub fn get_home_relay(config: &NodeConfig) -> Result<Option<HomeRelayConfig>> {
    Ok(config.load_settings()?.home_relay)
}

pub fn set_home_relay(
    config: &NodeConfig,
    multiaddr: &str,
    capability: HomeRelayCapability,
    api_url: Option<&str>,
) -> Result<HomeRelayConfig> {
    let (peer_id, _transport_addr) = parse_bootstrap_addr(multiaddr)
        .map_err(|e| anyhow::anyhow!("invalid home relay multiaddr: {e}"))?;

    let relay = HomeRelayConfig {
        peer_id: peer_id.to_string(),
        multiaddr: multiaddr.to_string(),
        capability,
        api_url: api_url.map(str::to_string),
    };

    let mut settings = config.load_settings()?;
    settings.home_relay = Some(relay.clone());
    config.save_settings(&settings)?;

    Ok(relay)
}

pub fn clear_home_relay(config: &NodeConfig) -> Result<HomeRelayChange> {
    let mut settings = config.load_settings()?;
    if settings.home_relay.is_none() {
        return Ok(HomeRelayChange::NotConfigured);
    }

    settings.home_relay = None;
    config.save_settings(&settings)?;
    Ok(HomeRelayChange::Cleared)
}

pub async fn show() -> Result<()> {
    let config = NodeConfig::default_dirs();
    print!("{}", format_home_relay(get_home_relay(&config)?.as_ref()));
    Ok(())
}

pub async fn set(
    multiaddr: &str,
    capability: HomeRelayCapabilityArg,
    api_url: Option<&str>,
) -> Result<()> {
    let config = NodeConfig::default_dirs();
    let relay = set_home_relay(&config, multiaddr, capability.into(), api_url)?;
    println!("Set home relay: {}", relay.multiaddr);
    println!("  Peer ID:    {}", relay.peer_id);
    println!("  Capability: {}", format_capability(&relay.capability));
    if let Some(api_url) = &relay.api_url {
        println!("  API URL:    {api_url}");
    }
    Ok(())
}

pub async fn clear() -> Result<()> {
    let config = NodeConfig::default_dirs();
    match clear_home_relay(&config)? {
        HomeRelayChange::Cleared => println!("Cleared home relay"),
        HomeRelayChange::NotConfigured => println!("Home relay is not configured"),
    }
    Ok(())
}

pub async fn pin(content_id: &str) -> Result<()> {
    let config = NodeConfig::default_dirs();
    let info = daemon::read_daemon_info(&config)
        .ok_or_else(|| anyhow::anyhow!("Daemon not running. Start with: dweb start"))?;

    if !daemon::is_daemon_running(&config) {
        daemon::clear_daemon_info(&config);
        anyhow::bail!("Daemon not running. Start with: dweb start");
    }

    let client = DaemonClient::new(info.port);
    let response = client.pin_to_home_relay(content_id, None).await?;
    let relay = response["relay"].as_str().unwrap_or("unknown");
    let latest_sequence = response["latest_sequence"].as_u64().unwrap_or(0);
    println!("Pinned {content_id} to home relay {relay}");
    println!("Update-log sequence: {latest_sequence}");
    Ok(())
}

fn format_home_relay(relay: Option<&HomeRelayConfig>) -> String {
    let Some(relay) = relay else {
        return "Home relay: not configured\n".to_string();
    };

    format!(
        "Home relay:\n  Multiaddr:  {}\n  Peer ID:    {}\n  Capability: {}\n  API URL:    {}\n",
        relay.multiaddr,
        relay.peer_id,
        format_capability(&relay.capability),
        relay.api_url.as_deref().unwrap_or("not configured")
    )
}

fn format_capability(capability: &HomeRelayCapability) -> &'static str {
    match capability {
        HomeRelayCapability::Unknown => "unknown",
        HomeRelayCapability::DiscoveryOnly => "discovery-only",
        HomeRelayCapability::Pinning => "pinning",
    }
}

impl From<HomeRelayCapabilityArg> for HomeRelayCapability {
    fn from(value: HomeRelayCapabilityArg) -> Self {
        match value {
            HomeRelayCapabilityArg::Unknown => Self::Unknown,
            HomeRelayCapabilityArg::DiscoveryOnly => Self::DiscoveryOnly,
            HomeRelayCapabilityArg::Pinning => Self::Pinning,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const RELAY: &str =
        "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

    fn test_config() -> (tempfile::TempDir, NodeConfig) {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        (dir, config)
    }

    #[test]
    fn set_show_and_clear_home_relay() {
        let (_dir, config) = test_config();

        let relay = set_home_relay(
            &config,
            RELAY,
            HomeRelayCapability::Pinning,
            Some("http://127.0.0.1:9862"),
        )
        .unwrap();

        assert_eq!(relay.multiaddr, RELAY);
        assert_eq!(
            relay.peer_id,
            "12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
        );
        assert_eq!(relay.capability, HomeRelayCapability::Pinning);
        assert_eq!(relay.api_url.as_deref(), Some("http://127.0.0.1:9862"));
        assert_eq!(get_home_relay(&config).unwrap(), Some(relay));

        assert_eq!(clear_home_relay(&config).unwrap(), HomeRelayChange::Cleared);
        assert_eq!(get_home_relay(&config).unwrap(), None);
    }

    #[test]
    fn set_home_relay_rejects_multiaddr_without_peer_id() {
        let (_dir, config) = test_config();

        let error = set_home_relay(
            &config,
            "/ip4/89.167.68.65/tcp/4001",
            HomeRelayCapability::Pinning,
            None,
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("invalid home relay multiaddr"));
        assert!(get_home_relay(&config).unwrap().is_none());
    }

    #[test]
    fn clear_home_relay_reports_missing_config() {
        let (_dir, config) = test_config();

        assert_eq!(
            clear_home_relay(&config).unwrap(),
            HomeRelayChange::NotConfigured
        );
    }

    #[test]
    fn formatted_home_relay_reports_configured_state() {
        let relay = HomeRelayConfig {
            peer_id: "peer".to_string(),
            multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/peer".to_string(),
            capability: HomeRelayCapability::DiscoveryOnly,
            api_url: None,
        };

        let output = format_home_relay(Some(&relay));

        assert!(output.contains("Home relay:"));
        assert!(output.contains("discovery-only"));
        assert!(output.contains("API URL:    not configured"));
    }
}
