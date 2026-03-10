use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::container::DockerConfigOverride;
use crate::env_vars::EnvValue;

const FILENAME: &str = "shade.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct LinkedRepo {
    pub name: String,
    /// Absolute host path to the clone in the shade dir.
    pub host_path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShadeConfig {
    #[serde(default)]
    pub env: HashMap<String, EnvValue>,
    #[serde(default)]
    pub docker: DockerConfigOverride,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub workspace_repos: Vec<LinkedRepo>,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let config = ShadeConfig::load(tmp.path()).unwrap();
        assert!(config.docker.image.is_none());
    }

    #[test]
    fn test_load_with_image() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(
            tmp.path().join("shade.toml"),
            "[docker]\nimage = \"node:20\"\n",
        )
        .unwrap();
        let config = ShadeConfig::load(tmp.path()).unwrap();
        assert_eq!(config.docker.image.as_deref(), Some("node:20"));
    }

    #[test]
    fn test_save_and_reload() {
        let tmp = TempDir::new().unwrap();
        let config = ShadeConfig {
            docker: DockerConfigOverride {
                image: Some("rust:latest".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        config.save(tmp.path()).unwrap();

        let loaded = ShadeConfig::load(tmp.path()).unwrap();
        assert_eq!(loaded.docker.image.as_deref(), Some("rust:latest"));
    }

    #[test]
    fn test_image_defaults_to_none() {
        let config = ShadeConfig::default();
        assert!(config.docker.image.is_none());
    }

    #[test]
    fn test_linked_repo_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let config = ShadeConfig {
            label: Some("my-feature".to_string()),
            workspace_repos: vec![
                LinkedRepo {
                    name: "core".to_string(),
                    host_path: "/home/user/Shades/2026-03-09-test/core".to_string(),
                },
                LinkedRepo {
                    name: "acme/dashboard".to_string(),
                    host_path: "/home/user/Shades/2026-03-09-test/acme/dashboard".to_string(),
                },
            ],
            ..Default::default()
        };
        config.save(tmp.path()).unwrap();

        let loaded = ShadeConfig::load(tmp.path()).unwrap();
        assert_eq!(loaded.label.as_deref(), Some("my-feature"));
        assert_eq!(loaded.workspace_repos, config.workspace_repos);
    }
}
