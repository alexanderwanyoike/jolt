use std::path::PathBuf;

pub struct NodeConfig {
    pub data_dir: PathBuf,
    pub identity_dir: PathBuf,
    pub content_store_dir: PathBuf,
}

impl NodeConfig {
    /// Create a config with the default platform-specific directories.
    pub fn default_dirs() -> Self {
        let base = directories::ProjectDirs::from("net", "dweb", "dweb")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                PathBuf::from(home).join(".dweb")
            });

        Self {
            identity_dir: base.join("identity"),
            content_store_dir: base.join("data").join("published"),
            data_dir: base,
        }
    }

    /// Create a config with a custom base directory.
    pub fn with_base_dir(base: PathBuf) -> Self {
        Self {
            identity_dir: base.join("identity"),
            content_store_dir: base.join("data").join("published"),
            data_dir: base,
        }
    }

    /// Ensure all required directories exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.identity_dir)?;
        std::fs::create_dir_all(&self.content_store_dir)?;
        Ok(())
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
    }

    #[test]
    fn default_dirs_produces_valid_paths() {
        let config = NodeConfig::default_dirs();
        assert!(config.data_dir.is_absolute() || config.data_dir.starts_with("."));
        assert!(config.identity_dir.starts_with(&config.data_dir));
        assert!(config.content_store_dir.starts_with(&config.data_dir));
    }
}
