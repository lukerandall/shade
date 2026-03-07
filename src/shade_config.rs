use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::container::ContainerLimitsOverride;
use crate::env_vars::EnvValue;

const FILENAME: &str = "shade.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShadeConfig {
    pub image: Option<String>,
    pub setup: Option<String>,
    #[serde(default)]
    pub mounts: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub container: ContainerLimitsOverride,
}

impl ShadeConfig {
    pub fn load(shade_dir: &Path) -> Result<Self> {
        let path = shade_dir.join(FILENAME);
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        toml::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn save(&self, shade_dir: &Path) -> Result<()> {
        let path = shade_dir.join(FILENAME);
        let contents = toml::to_string_pretty(self).context("failed to serialize shade config")?;
        std::fs::write(&path, contents)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    /// Resolve the image to use, falling back to the provided default.
    pub fn image_or(&self, default: &str) -> String {
        self.image.as_deref().unwrap_or(default).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let config = ShadeConfig::load(tmp.path()).unwrap();
        assert!(config.image.is_none());
    }

    #[test]
    fn test_load_with_image() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("shade.toml"), "image = \"node:20\"\n").unwrap();
        let config = ShadeConfig::load(tmp.path()).unwrap();
        assert_eq!(config.image.as_deref(), Some("node:20"));
    }

    #[test]
    fn test_save_and_reload() {
        let tmp = TempDir::new().unwrap();
        let config = ShadeConfig {
            image: Some("rust:latest".to_string()),
            ..Default::default()
        };
        config.save(tmp.path()).unwrap();

        let loaded = ShadeConfig::load(tmp.path()).unwrap();
        assert_eq!(loaded.image.as_deref(), Some("rust:latest"));
    }

    #[test]
    fn test_image_or_uses_override() {
        let config = ShadeConfig {
            image: Some("alpine:3".to_string()),
            ..Default::default()
        };
        assert_eq!(config.image_or("ubuntu:latest"), "alpine:3");
    }

    #[test]
    fn test_image_or_falls_back() {
        let config = ShadeConfig::default();
        assert_eq!(config.image_or("ubuntu:latest"), "ubuntu:latest");
    }
}
