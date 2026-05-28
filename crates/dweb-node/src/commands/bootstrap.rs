use anyhow::Result;
use dweb_network::bootstrap::{default_bootstrap_peers, parse_bootstrap_addr};

use crate::config::NodeConfig;

#[derive(Debug, Eq, PartialEq)]
pub struct BootstrapRelayList {
    pub configured: Vec<String>,
    pub built_in: Vec<String>,
    pub effective: Vec<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum BootstrapRelayChange {
    Added,
    AlreadyConfigured,
    Removed,
    NotConfigured,
}

pub fn list_relays(config: &NodeConfig) -> Result<BootstrapRelayList> {
    let settings = config.load_settings()?;
    let built_in = default_bootstrap_peers();
    let effective = settings.effective_bootstrap_relays(&[], &built_in);

    Ok(BootstrapRelayList {
        configured: settings.bootstrap_relays,
        built_in,
        effective,
    })
}

pub fn add_relay(config: &NodeConfig, multiaddr: &str) -> Result<BootstrapRelayChange> {
    validate_bootstrap_relay(multiaddr)?;

    let mut settings = config.load_settings()?;
    if settings
        .bootstrap_relays
        .iter()
        .any(|relay| relay == multiaddr)
    {
        return Ok(BootstrapRelayChange::AlreadyConfigured);
    }

    settings.bootstrap_relays.push(multiaddr.to_string());
    config.save_settings(&settings)?;
    Ok(BootstrapRelayChange::Added)
}

pub fn remove_relay(config: &NodeConfig, multiaddr: &str) -> Result<BootstrapRelayChange> {
    validate_bootstrap_relay(multiaddr)?;

    let mut settings = config.load_settings()?;
    let before = settings.bootstrap_relays.len();
    settings.bootstrap_relays.retain(|relay| relay != multiaddr);

    if settings.bootstrap_relays.len() == before {
        return Ok(BootstrapRelayChange::NotConfigured);
    }

    config.save_settings(&settings)?;
    Ok(BootstrapRelayChange::Removed)
}

pub async fn list() -> Result<()> {
    let config = NodeConfig::default_dirs();
    let relays = list_relays(&config)?;

    print!("{}", format_relay_list(&relays));

    Ok(())
}

pub async fn add(multiaddr: &str) -> Result<()> {
    let config = NodeConfig::default_dirs();
    match add_relay(&config, multiaddr)? {
        BootstrapRelayChange::Added => println!("Added bootstrap relay: {multiaddr}"),
        BootstrapRelayChange::AlreadyConfigured => {
            println!("Bootstrap relay already configured: {multiaddr}")
        }
        BootstrapRelayChange::Removed | BootstrapRelayChange::NotConfigured => unreachable!(),
    }
    Ok(())
}

pub async fn remove(multiaddr: &str) -> Result<()> {
    let config = NodeConfig::default_dirs();
    match remove_relay(&config, multiaddr)? {
        BootstrapRelayChange::Removed => println!("Removed bootstrap relay: {multiaddr}"),
        BootstrapRelayChange::NotConfigured => {
            println!("Bootstrap relay not configured: {multiaddr}")
        }
        BootstrapRelayChange::Added | BootstrapRelayChange::AlreadyConfigured => unreachable!(),
    }
    Ok(())
}

fn validate_bootstrap_relay(multiaddr: &str) -> Result<()> {
    parse_bootstrap_addr(multiaddr)
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("invalid bootstrap relay multiaddr: {e}"))
}

fn format_relay_list(relays: &BootstrapRelayList) -> String {
    let mut output = String::from("Bootstrap relays:\n");
    push_relay_group(&mut output, "Configured", &relays.configured);
    push_relay_group(&mut output, "Built-in defaults", &relays.built_in);
    push_relay_group(&mut output, "Effective at startup", &relays.effective);
    output
}

fn push_relay_group(output: &mut String, label: &str, relays: &[String]) {
    output.push_str(&format!("  {label}:\n"));
    if relays.is_empty() {
        output.push_str("    (none)\n");
        return;
    }

    for relay in relays {
        output.push_str(&format!("    {relay}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const RELAY_A: &str =
        "/ip4/89.167.68.65/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";
    const RELAY_B: &str =
        "/dns4/bootstrap.jolt.test/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN";

    fn test_config() -> (tempfile::TempDir, NodeConfig) {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        (dir, config)
    }

    #[test]
    fn add_lists_and_removes_configured_bootstrap_relays() {
        let (_dir, config) = test_config();

        assert_eq!(
            add_relay(&config, RELAY_A).unwrap(),
            BootstrapRelayChange::Added
        );
        assert_eq!(
            add_relay(&config, RELAY_B).unwrap(),
            BootstrapRelayChange::Added
        );

        let relays = list_relays(&config).unwrap();
        assert_eq!(relays.configured, vec![RELAY_A, RELAY_B]);
        assert_eq!(relays.effective, vec![RELAY_A, RELAY_B]);

        assert_eq!(
            remove_relay(&config, RELAY_A).unwrap(),
            BootstrapRelayChange::Removed
        );

        let relays = list_relays(&config).unwrap();
        assert_eq!(relays.configured, vec![RELAY_B]);
        assert_eq!(relays.effective, vec![RELAY_B]);
    }

    #[test]
    fn add_relay_rejects_malformed_multiaddr() {
        let (_dir, config) = test_config();

        let error = add_relay(&config, "/ip4/127.0.0.1/tcp/4001")
            .unwrap_err()
            .to_string();

        assert!(error.contains("invalid bootstrap relay multiaddr"));
        assert!(list_relays(&config).unwrap().configured.is_empty());
    }

    #[test]
    fn add_relay_does_not_duplicate_existing_multiaddr() {
        let (_dir, config) = test_config();

        assert_eq!(
            add_relay(&config, RELAY_A).unwrap(),
            BootstrapRelayChange::Added
        );
        assert_eq!(
            add_relay(&config, RELAY_A).unwrap(),
            BootstrapRelayChange::AlreadyConfigured
        );

        assert_eq!(list_relays(&config).unwrap().configured, vec![RELAY_A]);
    }

    #[test]
    fn remove_relay_reports_missing_multiaddr_without_changing_config() {
        let (_dir, config) = test_config();

        assert_eq!(
            add_relay(&config, RELAY_A).unwrap(),
            BootstrapRelayChange::Added
        );
        assert_eq!(
            remove_relay(&config, RELAY_B).unwrap(),
            BootstrapRelayChange::NotConfigured
        );

        assert_eq!(list_relays(&config).unwrap().configured, vec![RELAY_A]);
    }

    #[test]
    fn formatted_list_distinguishes_configured_builtin_and_effective_relays() {
        let relays = BootstrapRelayList {
            configured: vec![RELAY_A.to_string()],
            built_in: vec![RELAY_B.to_string()],
            effective: vec![RELAY_A.to_string()],
        };

        let output = format_relay_list(&relays);

        assert!(output.contains("Configured:"));
        assert!(output.contains(RELAY_A));
        assert!(output.contains("Built-in defaults:"));
        assert!(output.contains(RELAY_B));
        assert!(output.contains("Effective at startup:"));
    }
}
