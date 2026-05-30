use serde::{Deserialize, Serialize};

const DEFAULT_MAX_SIZE: u64 = 2 * 1024 * 1024 * 1024; // 2 GB

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheConfig {
    pub max_size_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_bytes: DEFAULT_MAX_SIZE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_2gb_max() {
        let config = CacheConfig::default();
        assert_eq!(config.max_size_bytes, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn config_serializes_to_json_roundtrip() {
        let config = CacheConfig {
            max_size_bytes: 500_000,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: CacheConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }
}
