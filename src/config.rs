use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("failed to read config file: {0}")]
    Read(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    env_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub env_dir: String,
}

impl Config {
    /// Load configuration from `~/.config/shade/config.toml`.
    ///
    /// If the file does not exist, returns defaults. If it exists but is
    /// malformed, returns an error.
    pub fn load() -> Result<Self> {
        let config_dir = dirs::config_dir()
            .context("could not determine config directory")?;
        let config_path = config_dir.join("shade").join("config.toml");
        Self::load_from(&config_path)
    }

    /// Load configuration from an arbitrary path (useful for testing).
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::defaults());
        }

        let contents = std::fs::read_to_string(path)
            .map_err(ConfigError::Read)
            .context("failed to read config file")?;

        let raw: RawConfig = toml::from_str(&contents)
            .map_err(ConfigError::Parse)
            .context("failed to parse config file")?;

        let env_dir = match raw.env_dir {
            Some(dir) => expand_tilde(&dir),
            None => Self::default_env_dir(),
        };

        Ok(Config { env_dir })
    }

    fn defaults() -> Self {
        Config {
            env_dir: Self::default_env_dir(),
        }
    }

    fn default_env_dir() -> String {
        let home = dirs::home_dir()
            .map(|h| h.join("Shade"))
            .unwrap_or_else(|| PathBuf::from("Shade"));
        home.to_string_lossy().to_string()
    }
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("~"))
            .to_string_lossy()
            .to_string();
    }

    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }

    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_defaults_when_no_config_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("nonexistent.toml");

        let config = Config::load_from(&config_path).unwrap();

        let expected_env_dir = dirs::home_dir().unwrap().join("Shade");
        assert_eq!(config.env_dir, expected_env_dir.to_string_lossy());
    }

    #[test]
    fn test_load_valid_config_with_custom_env_dir() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "env_dir = \"/custom/envs\"\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(config.env_dir, "/custom/envs");
    }

    #[test]
    fn test_tilde_expansion() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "env_dir = \"~/my-envs\"\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();

        let home = dirs::home_dir().unwrap();
        let expected = home.join("my-envs").to_string_lossy().to_string();
        assert_eq!(config.env_dir, expected);
    }

    #[test]
    fn test_malformed_config_returns_error() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "this is not valid [[ toml ===").unwrap();

        let result = Config::load_from(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_fields_use_defaults() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        // Empty but valid TOML — all fields should get defaults
        fs::write(&config_path, "").unwrap();

        let config = Config::load_from(&config_path).unwrap();

        let expected_env_dir = dirs::home_dir().unwrap().join("Shade");
        assert_eq!(config.env_dir, expected_env_dir.to_string_lossy());
    }
}
