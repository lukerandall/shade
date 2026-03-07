use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::container::ContainerLimits;
use crate::env_vars::EnvValue;

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
    code_dirs: Option<Vec<String>>,
    default_image: Option<String>,
    setup: Option<String>,
    keychain_prefix: Option<String>,
    #[serde(default)]
    mounts: Vec<String>,
    #[serde(default)]
    env: HashMap<String, EnvValue>,
    #[serde(default)]
    container: ContainerLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub env_dir: String,
    pub code_dirs: Vec<String>,
    pub default_image: String,
    pub setup: Option<String>,
    pub keychain_prefix: String,
    pub mounts: Vec<String>,
    pub env: HashMap<String, EnvValue>,
    pub container: ContainerLimits,
}

impl Config {
    /// Load configuration from `~/.config/shade/config.toml`.
    ///
    /// If the file does not exist, returns defaults. If it exists but is
    /// malformed, returns an error.
    pub fn load() -> Result<Self> {
        // Prefer ~/.config (XDG) over platform default (~/Library/Application Support on macOS)
        let xdg_path =
            dirs::home_dir().map(|h| h.join(".config").join("shade").join("config.toml"));
        if let Some(ref path) = xdg_path
            && path.exists()
        {
            return Self::load_from(path);
        }

        let config_dir = dirs::config_dir().context("could not determine config directory")?;
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

        let code_dirs = match raw.code_dirs {
            Some(dirs) => dirs.iter().map(|d| expand_tilde(d)).collect(),
            None => Self::default_code_dirs(),
        };

        let default_image = raw.default_image.unwrap_or_else(Self::default_image);
        let keychain_prefix = raw
            .keychain_prefix
            .unwrap_or_else(Self::default_keychain_prefix);

        let mounts = raw.mounts.iter().map(|m| expand_tilde(m)).collect();

        Ok(Config {
            env_dir,
            code_dirs,
            default_image,
            setup: raw.setup,
            keychain_prefix,
            mounts,
            env: raw.env,
            container: raw.container,
        })
    }

    fn defaults() -> Self {
        Config {
            env_dir: Self::default_env_dir(),
            code_dirs: Self::default_code_dirs(),
            default_image: Self::default_image(),
            setup: None,
            keychain_prefix: Self::default_keychain_prefix(),
            mounts: Vec::new(),
            env: HashMap::new(),
            container: ContainerLimits::default(),
        }
    }

    fn default_image() -> String {
        "ubuntu:latest".to_string()
    }

    fn default_keychain_prefix() -> String {
        "shade.".to_string()
    }

    const DEFAULT_ENV_DIR: &str = "~/Shades";
    const DEFAULT_CODE_DIRS: &[&str] = &["~/Code"];

    /// Generate a default config file as a TOML string with human-friendly paths.
    pub fn generate_default() -> String {
        #[derive(Serialize)]
        struct FileConfig {
            env_dir: String,
            code_dirs: Vec<String>,
            default_image: String,
            keychain_prefix: String,
        }

        let config = FileConfig {
            env_dir: Self::DEFAULT_ENV_DIR.to_string(),
            code_dirs: Self::DEFAULT_CODE_DIRS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            default_image: Self::default_image(),
            keychain_prefix: Self::default_keychain_prefix(),
        };

        toml::to_string_pretty(&config).expect("failed to serialize default config")
    }

    /// Return the default config file path.
    pub fn default_path() -> PathBuf {
        dirs::home_dir()
            .map(|h| h.join(".config").join("shade").join("config.toml"))
            .unwrap_or_else(|| PathBuf::from(".config/shade/config.toml"))
    }

    fn default_code_dirs() -> Vec<String> {
        Self::DEFAULT_CODE_DIRS
            .iter()
            .map(|d| expand_tilde(d))
            .collect()
    }

    fn default_env_dir() -> String {
        expand_tilde(Self::DEFAULT_ENV_DIR)
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

    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
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

        let home = dirs::home_dir().unwrap();
        assert_eq!(config.env_dir, home.join("Shades").to_string_lossy());
        assert_eq!(
            config.code_dirs,
            vec![home.join("Code").to_string_lossy().to_string()]
        );
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

        let home = dirs::home_dir().unwrap();
        assert_eq!(config.env_dir, home.join("Shades").to_string_lossy());
        assert_eq!(
            config.code_dirs,
            vec![home.join("Code").to_string_lossy().to_string()]
        );
    }

    #[test]
    fn test_custom_code_dirs() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "code_dirs = [\"/projects\", \"~/work\"]\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();

        let home = dirs::home_dir().unwrap();
        assert_eq!(config.code_dirs[0], "/projects");
        assert_eq!(
            config.code_dirs[1],
            home.join("work").to_string_lossy().to_string()
        );
    }
}
