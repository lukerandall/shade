use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use thiserror::Error;

use crate::container::DockerConfig;
use crate::env_vars::EnvValue;
use crate::vcs::{LinkMode, VcsKind};

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
    vcs: Option<VcsKind>,
    link_mode: Option<LinkMode>,
    #[serde(default)]
    init_repo: bool,
    default_shade_setup: Option<String>,
    keychain_prefix: Option<String>,
    #[serde(default)]
    env: HashMap<String, EnvValue>,
    #[serde(default)]
    docker: DockerConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub env_dir: String,
    pub code_dirs: Vec<String>,
    pub vcs_kind: VcsKind,
    pub link_mode: LinkMode,
    pub init_repo: bool,
    pub default_shade_setup: Option<String>,
    pub keychain_prefix: String,
    pub env: HashMap<String, EnvValue>,
    pub docker: DockerConfig,
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

        let keychain_prefix = raw
            .keychain_prefix
            .unwrap_or_else(Self::default_keychain_prefix);

        let docker = DockerConfig {
            mounts: raw
                .docker
                .mounts
                .iter()
                .map(|m| expand_tilde_mount_source(m))
                .collect(),
            ..raw.docker
        };

        let vcs_kind = raw.vcs.unwrap_or_default();
        let link_mode = raw.link_mode.unwrap_or_default();

        Ok(Config {
            env_dir,
            code_dirs,
            vcs_kind,
            link_mode,
            init_repo: raw.init_repo,
            default_shade_setup: raw.default_shade_setup,
            keychain_prefix,
            env: raw.env,
            docker,
        })
    }

    fn defaults() -> Self {
        Config {
            env_dir: Self::default_env_dir(),
            code_dirs: Self::default_code_dirs(),
            vcs_kind: VcsKind::default(),
            link_mode: LinkMode::default(),
            init_repo: false,
            default_shade_setup: None,
            keychain_prefix: Self::default_keychain_prefix(),
            env: HashMap::new(),
            docker: DockerConfig::default(),
        }
    }

    fn default_keychain_prefix() -> String {
        "shade.".to_string()
    }

    const DEFAULT_ENV_DIR: &str = "~/Shades";
    const DEFAULT_CODE_DIRS: &[&str] = &[];

    /// Generate a default config file as a TOML string with human-friendly paths.
    pub fn generate_default() -> String {
        format!(
            r##"# Directory where shade environments are created.
env_dir = "{env_dir}"

# Directories to scan for repositories when creating a shade. Discovered repos
# are listed in an interactive picker so you can link them into the new shade.
# If empty or unset, the repo selection step is skipped entirely.
# code_dirs = ["~/Code"]

# Version control system: "jj" (Jujutsu) or "git".
# vcs = "jj"

# How repos are linked into shades: "workspace" (shared history, lightweight)
# or "clone" (independent copy, safer for untrusted agents).
# link_mode = "workspace"

# Initialize a new repo in each shade directory on creation.
# init_repo = false

# Default shade_setup command copied into new shade.toml files.
# Runs as the configured user at container start; re-runs when changed.
# default_shade_setup = """
#   curl -fsSL https://example.com/install.sh | bash
# """

# Prefix applied to keychain service names (e.g. "shade.my-token").
keychain_prefix = "{keychain_prefix}"

# Environment variables injected into every shade container.
# [env]
# STATIC_VAR = "value"
# FROM_KEYCHAIN = {{ keychain = "service-name" }}
# FROM_COMMAND  = {{ command = "cat ~/.secrets/token" }}

[docker]
# Base Docker image.
image = "ubuntu:latest"

# Run the container as this user.
# user = "dev"

# Terminal multiplexer to install and use ("zellij" or "tmux").
# multiplexer = "zellij"

# Shell command baked into the Docker image during `shade docker build`.
# Runs as root. Rebuild with `shade docker build` after changing.
# base_image_setup = "apt-get update && apt-get install -y ripgrep"

# Extra directories to prepend to PATH inside the container.
# path = ["/home/dev/.cargo/bin"]

# Additional host paths to mount into the container.
# Use "source:target" for explicit mapping. Tilde (~) in sources expands
# to the host user's home directory; in targets it expands to the container
# user's home directory.
# mounts = ["~/.ssh", "~/.config:~/.config"]

# [docker.limits]
# memory    = "4g"
# cpus      = "2"
# pids_limit = "256"
# cap_drop  = ["ALL"]
# cap_add   = ["SETUID", "SETGID"]
# no_new_privileges = true
"##,
            env_dir = Self::DEFAULT_ENV_DIR,
            keychain_prefix = Self::default_keychain_prefix(),
        )
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

/// Expand tilde only in the source (host) side of a mount spec.
/// For "source:target" mounts, only the source is expanded here;
/// the target is expanded at container creation time using the container
/// user's home directory.
fn expand_tilde_mount_source(mount: &str) -> String {
    if let Some((src, tgt)) = mount.split_once(':') {
        format!("{}:{}", expand_tilde(src), tgt)
    } else {
        expand_tilde(mount)
    }
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
        assert!(config.code_dirs.is_empty());
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

        fs::write(&config_path, "").unwrap();

        let config = Config::load_from(&config_path).unwrap();

        let home = dirs::home_dir().unwrap();
        assert_eq!(config.env_dir, home.join("Shades").to_string_lossy());
        assert!(config.code_dirs.is_empty());
    }

    #[test]
    fn test_link_mode_defaults_to_workspace() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(config.link_mode, LinkMode::Workspace);
    }

    #[test]
    fn test_link_mode_clone() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "link_mode = \"clone\"\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(config.link_mode, LinkMode::Clone);
    }

    #[test]
    fn test_vcs_git_with_clone() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "vcs = \"git\"\nlink_mode = \"clone\"\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(config.vcs_kind, VcsKind::Git);
        assert_eq!(config.link_mode, LinkMode::Clone);
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

    #[test]
    fn test_init_repo_defaults_to_false() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        fs::write(&config_path, "").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert!(!config.init_repo);
    }

    #[test]
    fn test_init_repo_enabled() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        fs::write(&config_path, "init_repo = true\n").unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert!(config.init_repo);
    }

    #[test]
    fn test_mount_tilde_expands_source_only() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        fs::write(
            &config_path,
            "[docker]\nmounts = [\"~/.claude.json:~/.claude.json\"]\n",
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        let home = dirs::home_dir().unwrap();
        let mount = &config.docker.mounts[0];
        let (src, tgt) = mount.split_once(':').unwrap();
        // Source should be expanded
        assert!(src.starts_with(home.to_str().unwrap()));
        assert!(src.ends_with(".claude.json"));
        // Target should NOT be expanded (still has tilde)
        assert_eq!(tgt, "~/.claude.json");
    }

    #[test]
    fn test_default_shade_setup() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");
        fs::write(
            &config_path,
            "default_shade_setup = \"curl -fsSL https://example.com | bash\"\n",
        )
        .unwrap();

        let config = Config::load_from(&config_path).unwrap();
        assert_eq!(
            config.default_shade_setup.as_deref(),
            Some("curl -fsSL https://example.com | bash")
        );
    }
}
