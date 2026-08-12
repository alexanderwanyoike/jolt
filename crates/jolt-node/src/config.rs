use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};

use jolt_core::IdentityId;
use jolt_network::{HomeRelayConfig, RelayPinPolicy};

#[derive(Clone)]
pub struct NodeConfig {
    pub data_dir: PathBuf,
    pub identity_dir: PathBuf,
    pub content_store_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub settings_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeSettings {
    #[serde(default)]
    pub bootstrap_relays: Vec<String>,
    #[serde(default = "default_use_builtin_bootstrap_relays")]
    pub use_builtin_bootstrap_relays: bool,
    #[serde(default)]
    pub bootstrap_relay: bool,
    #[serde(default)]
    pub home_relay: Option<HomeRelayConfig>,
    #[serde(default)]
    pub relay_pin_policy: RelayPinPolicy,
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            bootstrap_relays: Vec::new(),
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: None,
            relay_pin_policy: RelayPinPolicy::default(),
        }
    }
}

impl NodeSettings {
    pub fn update_relay_pin_allowlist(&mut self, identities: &[String], reset: bool) -> Result<()> {
        let identities = canonical_relay_pin_identities(identities.iter().map(String::as_str))?;

        if reset {
            self.relay_pin_policy.allowed_identities.clear();
        }
        self.relay_pin_policy.allowed_identities.extend(identities);
        Ok(())
    }

    fn canonicalize_relay_pin_allowlist(&mut self) -> Result<()> {
        self.relay_pin_policy.allowed_identities = canonical_relay_pin_identities(
            self.relay_pin_policy
                .allowed_identities
                .iter()
                .map(String::as_str),
        )?;
        Ok(())
    }

    pub fn effective_bootstrap_relays(
        &self,
        cli_bootstrap_relays: &[String],
        builtin_bootstrap_relays: &[String],
    ) -> Vec<String> {
        let mut relays = Vec::new();
        for relay in self
            .bootstrap_relays
            .iter()
            .chain(cli_bootstrap_relays.iter())
        {
            if !relays.contains(relay) {
                relays.push(relay.clone());
            }
        }

        if relays.is_empty() && self.use_builtin_bootstrap_relays {
            for relay in builtin_bootstrap_relays {
                if !relays.contains(relay) {
                    relays.push(relay.clone());
                }
            }
        }

        relays
    }
}

fn canonical_relay_pin_identities<'a>(
    identities: impl IntoIterator<Item = &'a str>,
) -> Result<BTreeSet<String>> {
    identities
        .into_iter()
        .map(canonical_relay_pin_identity)
        .collect()
}

fn canonical_relay_pin_identity(raw: &str) -> Result<String> {
    if raw.contains('/') {
        bail!(
            "relay pin allowlist entry must be an identity, not a content path: {raw}; use <identity>.jolt"
        );
    }

    let label = raw.strip_suffix(".jolt").unwrap_or(raw);
    IdentityId::from_str(label)
        .map(|identity| identity.to_string())
        .map_err(|error| anyhow!("invalid relay pin identity '{raw}': {error}"))
}

fn default_use_builtin_bootstrap_relays() -> bool {
    true
}

impl NodeConfig {
    /// Create a config with the default platform-specific directories.
    pub fn default_dirs() -> Self {
        let base = directories::ProjectDirs::from("net", "jolt", "jolt")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".jolt")
            });

        Self {
            identity_dir: base.join("identity"),
            content_store_dir: base.join("data"),
            cache_dir: base.join("data").join("cache"),
            settings_path: base.join("config.json"),
            data_dir: base,
        }
    }

    /// Create a config with a custom base directory.
    pub fn with_base_dir(base: PathBuf) -> Self {
        Self {
            identity_dir: base.join("identity"),
            content_store_dir: base.join("data"),
            cache_dir: base.join("data").join("cache"),
            settings_path: base.join("config.json"),
            data_dir: base,
        }
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.identity_dir)?;
        std::fs::create_dir_all(&self.content_store_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        Ok(())
    }

    pub fn load_settings(&self) -> std::io::Result<NodeSettings> {
        match std::fs::read_to_string(&self.settings_path) {
            Ok(raw) => {
                let mut settings: NodeSettings =
                    serde_json::from_str(&raw).map_err(std::io::Error::other)?;
                settings
                    .canonicalize_relay_pin_allowlist()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
                Ok(settings)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NodeSettings::default()),
            Err(e) => Err(e),
        }
    }

    pub fn save_settings(&self, settings: &NodeSettings) -> std::io::Result<()> {
        if let Some(parent) = self.settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        std::fs::write(&self.settings_path, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn identity_label() -> String {
        jolt_identity::NodeIdentity::generate()
            .identity_id()
            .to_string()
    }

    #[test]
    fn ensure_dirs_creates_directories() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        config.ensure_dirs().unwrap();

        assert!(config.identity_dir.exists());
        assert!(config.content_store_dir.exists());
        assert!(config.cache_dir.exists());
    }

    #[test]
    fn default_dirs_produces_valid_paths() {
        let config = NodeConfig::default_dirs();
        assert!(config.identity_dir.starts_with(&config.data_dir));
        assert!(config.content_store_dir.starts_with(&config.data_dir));
        assert!(config.cache_dir.starts_with(&config.data_dir));
        assert!(config.settings_path.starts_with(&config.data_dir));
    }

    #[test]
    fn config_has_cache_dir() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        assert!(config.cache_dir.to_string_lossy().contains("cache"));
    }

    #[test]
    fn node_settings_round_trip_bootstrap_relays_and_relay_mode() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        config.ensure_dirs().unwrap();
        let settings = NodeSettings {
            bootstrap_relays: vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWExample".to_string()],
            use_builtin_bootstrap_relays: false,
            bootstrap_relay: true,
            home_relay: Some(HomeRelayConfig {
                peer_id: "12D3KooWExample".to_string(),
                multiaddr: "/ip4/127.0.0.1/tcp/4001/p2p/12D3KooWExample".to_string(),
                capability: jolt_network::HomeRelayCapability::Pinning,
                api_url: Some("http://127.0.0.1:9863".to_string()),
            }),
            relay_pin_policy: RelayPinPolicy::default(),
        };

        config.save_settings(&settings).unwrap();
        let loaded = config.load_settings().unwrap();

        assert_eq!(loaded, settings);
    }

    #[test]
    fn missing_node_settings_loads_defaults() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());

        let settings = config.load_settings().unwrap();

        assert_eq!(settings, NodeSettings::default());
    }

    #[test]
    fn load_settings_canonicalizes_manually_configured_root_identity() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        config.ensure_dirs().unwrap();
        let identity = identity_label();
        std::fs::write(
            &config.settings_path,
            serde_json::json!({
                "relay_pin_policy": {
                    "allowed_identities": [format!("{identity}.jolt")]
                }
            })
            .to_string(),
        )
        .unwrap();

        let settings = config.load_settings().unwrap();

        assert_eq!(
            settings.relay_pin_policy.allowed_identities,
            [identity].into()
        );
    }

    #[test]
    fn load_settings_rejects_manually_configured_content_path() {
        let dir = tempdir().unwrap();
        let config = NodeConfig::with_base_dir(dir.path().to_path_buf());
        config.ensure_dirs().unwrap();
        let identity = identity_label();
        std::fs::write(
            &config.settings_path,
            serde_json::json!({
                "relay_pin_policy": {
                    "allowed_identities": [format!("{identity}.jolt/canary")]
                }
            })
            .to_string(),
        )
        .unwrap();

        let error = config.load_settings().unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error
            .to_string()
            .contains("allowlist entry must be an identity"));
    }

    #[test]
    fn relay_pin_allowlist_updates_preserve_existing_identities() {
        let alice = identity_label();
        let bob = identity_label();
        let mut settings = NodeSettings::default();
        settings
            .relay_pin_policy
            .allowed_identities
            .insert(alice.clone());

        settings
            .update_relay_pin_allowlist(&[format!("{bob}.jolt")], false)
            .unwrap();

        assert_eq!(
            settings.relay_pin_policy.allowed_identities,
            [alice, bob].into()
        );
    }

    #[test]
    fn relay_pin_allowlist_reset_is_explicit() {
        let alice = identity_label();
        let bob = identity_label();
        let mut settings = NodeSettings::default();
        settings.relay_pin_policy.allowed_identities.insert(alice);

        settings
            .update_relay_pin_allowlist(&[format!("{bob}.jolt")], true)
            .unwrap();

        assert_eq!(settings.relay_pin_policy.allowed_identities, [bob].into());
    }

    #[test]
    fn relay_pin_allowlist_rejects_path_bearing_identity_without_mutating_policy() {
        let existing = identity_label();
        let requested = identity_label();
        let mut settings = NodeSettings::default();
        settings
            .relay_pin_policy
            .allowed_identities
            .insert(existing.clone());

        let result =
            settings.update_relay_pin_allowlist(&[format!("{requested}.jolt/canary/alice")], true);

        assert!(result.is_err());
        assert_eq!(
            settings.relay_pin_policy.allowed_identities,
            [existing].into()
        );
    }

    #[test]
    fn relay_pin_allowlist_rejects_non_canonical_identity() {
        let identity = identity_label().to_ascii_uppercase();
        let mut settings = NodeSettings::default();

        let error = settings
            .update_relay_pin_allowlist(&[identity], false)
            .unwrap_err();

        assert!(error.to_string().contains("invalid relay pin identity"));
        assert!(settings.relay_pin_policy.allowed_identities.is_empty());
    }

    #[test]
    fn effective_bootstrap_relays_use_explicit_then_defaults() {
        let settings = NodeSettings {
            bootstrap_relays: vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string()],
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: None,
            relay_pin_policy: RelayPinPolicy::default(),
        };
        let cli = vec!["/ip4/127.0.0.1/tcp/4002/p2p/12D3Cli".to_string()];
        let defaults = vec!["/dns4/bootstrap.jolt.test/tcp/4001/p2p/12D3Default".to_string()];

        assert_eq!(
            settings.effective_bootstrap_relays(&cli, &defaults),
            vec![
                "/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string(),
                "/ip4/127.0.0.1/tcp/4002/p2p/12D3Cli".to_string(),
            ]
        );

        assert_eq!(
            NodeSettings::default().effective_bootstrap_relays(&[], &defaults),
            defaults
        );
    }
}
