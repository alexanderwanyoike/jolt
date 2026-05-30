use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use jolt_network::HomeRelayConfig;

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
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            bootstrap_relays: Vec::new(),
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: None,
        }
    }
}

impl NodeSettings {
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
            Ok(raw) => serde_json::from_str(&raw).map_err(std::io::Error::other),
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
    fn effective_bootstrap_relays_use_explicit_then_defaults() {
        let settings = NodeSettings {
            bootstrap_relays: vec!["/ip4/127.0.0.1/tcp/4001/p2p/12D3Configured".to_string()],
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: None,
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
