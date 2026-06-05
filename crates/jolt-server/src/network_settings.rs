use std::path::PathBuf;

use jolt_network::{
    bootstrap::{default_bootstrap_peers, parse_bootstrap_addr},
    HomeRelayCapability, HomeRelayConfig,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone)]
pub struct NetworkSettingsStore {
    settings_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NetworkSettings {
    #[serde(default)]
    pub bootstrap_relays: Vec<String>,
    #[serde(default = "default_use_builtin_bootstrap_relays")]
    pub use_builtin_bootstrap_relays: bool,
    #[serde(default)]
    pub bootstrap_relay: bool,
    #[serde(default)]
    pub home_relay: Option<HomeRelayConfig>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkSettingsResponse {
    pub configured_bootstrap_relays: Vec<String>,
    pub built_in_bootstrap_relays: Vec<String>,
    pub effective_bootstrap_relays: Vec<String>,
    pub configured_bootstrap_relay_count: usize,
    pub built_in_bootstrap_relay_count: usize,
    pub effective_bootstrap_relay_count: usize,
    pub use_builtin_bootstrap_relays: bool,
    pub bootstrap_relay: bool,
    pub home_relay: Option<HomeRelayConfig>,
}

impl Default for NetworkSettings {
    fn default() -> Self {
        Self {
            bootstrap_relays: Vec::new(),
            use_builtin_bootstrap_relays: true,
            bootstrap_relay: false,
            home_relay: None,
        }
    }
}

impl NetworkSettings {
    pub fn effective_bootstrap_relays(&self, built_in: &[String]) -> Vec<String> {
        let mut relays = Vec::new();
        for relay in &self.bootstrap_relays {
            if !relays.contains(relay) {
                relays.push(relay.clone());
            }
        }

        if relays.is_empty() && self.use_builtin_bootstrap_relays {
            for relay in built_in {
                if !relays.contains(relay) {
                    relays.push(relay.clone());
                }
            }
        }

        relays
    }
}

impl NetworkSettingsStore {
    pub fn open(settings_path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self { settings_path })
    }

    pub fn open_default() -> std::io::Result<Self> {
        let base = directories::ProjectDirs::from("net", "jolt", "jolt")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".jolt")
            });
        Self::open(base.join("config.json"))
    }

    pub fn response(&self) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
        response_from_settings(self.load()?)
    }

    pub fn add_bootstrap_relay(
        &self,
        multiaddr: &str,
    ) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
        validate_relay_multiaddr(multiaddr, "bootstrap relay")?;
        let mut settings = self.load()?;
        if !settings
            .bootstrap_relays
            .iter()
            .any(|relay| relay == multiaddr)
        {
            settings.bootstrap_relays.push(multiaddr.to_string());
            self.save(&settings)?;
        }
        response_from_settings(settings)
    }

    pub fn remove_bootstrap_relay(
        &self,
        multiaddr: &str,
    ) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
        validate_relay_multiaddr(multiaddr, "bootstrap relay")?;
        let mut settings = self.load()?;
        settings.bootstrap_relays.retain(|relay| relay != multiaddr);
        self.save(&settings)?;
        response_from_settings(settings)
    }

    pub fn set_home_relay(
        &self,
        multiaddr: &str,
        capability: HomeRelayCapability,
        api_url: Option<&str>,
    ) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
        let (peer_id, _) = validate_relay_multiaddr(multiaddr, "home relay")?;
        let api_url = validate_api_url(api_url)?;
        let mut settings = self.load()?;
        settings.home_relay = Some(HomeRelayConfig {
            peer_id,
            multiaddr: multiaddr.to_string(),
            capability,
            api_url,
        });
        self.save(&settings)?;
        response_from_settings(settings)
    }

    pub fn clear_home_relay(&self) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
        let mut settings = self.load()?;
        settings.home_relay = None;
        self.save(&settings)?;
        response_from_settings(settings)
    }

    fn load(&self) -> Result<NetworkSettings, NetworkSettingsError> {
        let object = self.load_object()?;
        serde_json::from_value(Value::Object(object))
            .map_err(|error| NetworkSettingsError::Storage(error.to_string()))
    }

    fn save(&self, settings: &NetworkSettings) -> Result<(), NetworkSettingsError> {
        let mut object = self.load_object()?;
        object.insert(
            "bootstrap_relays".to_string(),
            serde_json::to_value(&settings.bootstrap_relays)
                .map_err(|error| NetworkSettingsError::Storage(error.to_string()))?,
        );
        object.insert(
            "use_builtin_bootstrap_relays".to_string(),
            Value::Bool(settings.use_builtin_bootstrap_relays),
        );
        object.insert(
            "bootstrap_relay".to_string(),
            Value::Bool(settings.bootstrap_relay),
        );
        object.insert(
            "home_relay".to_string(),
            serde_json::to_value(&settings.home_relay)
                .map_err(|error| NetworkSettingsError::Storage(error.to_string()))?,
        );

        if let Some(parent) = self.settings_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| NetworkSettingsError::Storage(error.to_string()))?;
        }
        let raw = serde_json::to_string_pretty(&Value::Object(object))
            .map_err(|error| NetworkSettingsError::Storage(error.to_string()))?;
        std::fs::write(&self.settings_path, raw)
            .map_err(|error| NetworkSettingsError::Storage(error.to_string()))
    }

    fn load_object(&self) -> Result<Map<String, Value>, NetworkSettingsError> {
        match std::fs::read_to_string(&self.settings_path) {
            Ok(raw) => match serde_json::from_str::<Value>(&raw)
                .map_err(|error| NetworkSettingsError::Storage(error.to_string()))?
            {
                Value::Object(object) => Ok(object),
                _ => Err(NetworkSettingsError::Storage(
                    "network settings file must be a JSON object".to_string(),
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(error) => Err(NetworkSettingsError::Storage(error.to_string())),
        }
    }
}

#[derive(Debug)]
pub enum NetworkSettingsError {
    Invalid(String),
    Storage(String),
}

impl std::fmt::Display for NetworkSettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) | Self::Storage(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for NetworkSettingsError {}

fn response_from_settings(
    settings: NetworkSettings,
) -> Result<NetworkSettingsResponse, NetworkSettingsError> {
    let built_in = default_bootstrap_peers();
    let effective = settings.effective_bootstrap_relays(&built_in);
    Ok(NetworkSettingsResponse {
        configured_bootstrap_relay_count: settings.bootstrap_relays.len(),
        built_in_bootstrap_relay_count: built_in.len(),
        effective_bootstrap_relay_count: effective.len(),
        configured_bootstrap_relays: settings.bootstrap_relays,
        built_in_bootstrap_relays: built_in,
        effective_bootstrap_relays: effective,
        use_builtin_bootstrap_relays: settings.use_builtin_bootstrap_relays,
        bootstrap_relay: settings.bootstrap_relay,
        home_relay: settings.home_relay,
    })
}

fn validate_relay_multiaddr(
    multiaddr: &str,
    label: &str,
) -> Result<(String, jolt_network::Multiaddr), NetworkSettingsError> {
    parse_bootstrap_addr(multiaddr)
        .map(|(peer_id, transport)| (peer_id.to_string(), transport))
        .map_err(|error| {
            NetworkSettingsError::Invalid(format!("invalid {label} multiaddr: {error}"))
        })
}

fn validate_api_url(api_url: Option<&str>) -> Result<Option<String>, NetworkSettingsError> {
    let Some(api_url) = api_url.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let parsed = reqwest::Url::parse(api_url).map_err(|error| {
        NetworkSettingsError::Invalid(format!("invalid home relay API URL: {error}"))
    })?;
    match parsed.scheme() {
        "http" | "https" => Ok(Some(api_url.to_string())),
        scheme => Err(NetworkSettingsError::Invalid(format!(
            "invalid home relay API URL scheme: {scheme}"
        ))),
    }
}

fn default_use_builtin_bootstrap_relays() -> bool {
    true
}
