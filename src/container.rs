use serde::{Deserialize, Serialize};

use crate::multiplexer::MultiplexerKind;

/// How repos are set up inside the Docker container.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepoMode {
    /// Create a VCS workspace/worktree inside the container from the primary repo.
    #[default]
    Workspace,
    /// Mount the repo directly into the container.
    Direct,
}

fn default_image() -> String {
    "ubuntu:latest".to_string()
}

fn default_memory() -> String {
    "4g".to_string()
}

fn default_cpus() -> String {
    "2".to_string()
}

fn default_pids_limit() -> String {
    "256".to_string()
}

fn default_cap_drop() -> Vec<String> {
    vec!["ALL".to_string()]
}

fn default_cap_add() -> Vec<String> {
    vec!["SETUID".to_string(), "SETGID".to_string()]
}

fn default_no_new_privileges() -> bool {
    true
}

/// Docker configuration from the root config `[docker]` section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DockerConfig {
    #[serde(default = "default_image")]
    pub image: String,
    /// Shell command baked into the Docker image during `shade docker build`.
    /// Runs as root. Rebuild with `shade docker build` after changing.
    pub base_image_setup: Option<String>,
    /// Shell command baked into the Docker image during `shade docker build`.
    /// Runs as the configured user. Rebuild with `shade docker build` after changing.
    pub base_image_user_setup: Option<String>,
    /// Shell command that runs as the configured user at container start.
    /// Re-runs when changed (content-hashed).
    pub shade_setup: Option<String>,
    pub user: Option<String>,
    pub multiplexer: Option<MultiplexerKind>,
    /// How repos are set up inside the container: "workspace" (default) or "direct".
    #[serde(default)]
    pub repo_mode: RepoMode,
    #[serde(default)]
    pub path: Vec<String>,
    #[serde(default)]
    pub mounts: Vec<String>,
    #[serde(default)]
    pub limits: ContainerLimits,
}

impl Default for DockerConfig {
    fn default() -> Self {
        Self {
            image: default_image(),
            base_image_setup: None,
            base_image_user_setup: None,
            shade_setup: None,
            user: None,
            multiplexer: None,
            repo_mode: RepoMode::default(),
            path: Vec::new(),
            mounts: Vec::new(),
            limits: ContainerLimits::default(),
        }
    }
}

/// Docker overrides from the per-shade `[docker]` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DockerConfigOverride {
    pub image: Option<String>,
    pub base_image_setup: Option<String>,
    pub base_image_user_setup: Option<String>,
    pub shade_setup: Option<String>,
    pub user: Option<String>,
    pub multiplexer: Option<MultiplexerKind>,
    pub repo_mode: Option<RepoMode>,
    pub path: Option<Vec<String>>,
    pub mounts: Option<Vec<String>>,
    #[serde(default)]
    pub limits: ContainerLimitsOverride,
}

impl DockerConfig {
    pub fn merge(&self, overrides: &DockerConfigOverride) -> Self {
        Self {
            image: overrides
                .image
                .clone()
                .unwrap_or_else(|| self.image.clone()),
            base_image_setup: overrides
                .base_image_setup
                .clone()
                .or_else(|| self.base_image_setup.clone()),
            base_image_user_setup: overrides
                .base_image_user_setup
                .clone()
                .or_else(|| self.base_image_user_setup.clone()),
            shade_setup: overrides
                .shade_setup
                .clone()
                .or_else(|| self.shade_setup.clone()),
            user: overrides.user.clone().or_else(|| self.user.clone()),
            multiplexer: overrides
                .multiplexer
                .clone()
                .or_else(|| self.multiplexer.clone()),
            repo_mode: overrides.repo_mode.unwrap_or(self.repo_mode),
            path: overrides.path.clone().unwrap_or_else(|| self.path.clone()),
            mounts: overrides
                .mounts
                .clone()
                .unwrap_or_else(|| self.mounts.clone()),
            limits: self.limits.merge(&overrides.limits),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContainerLimits {
    #[serde(default = "default_memory")]
    pub memory: String,
    #[serde(default = "default_cpus")]
    pub cpus: String,
    #[serde(default = "default_pids_limit")]
    pub pids_limit: String,
    #[serde(default = "default_cap_drop")]
    pub cap_drop: Vec<String>,
    #[serde(default = "default_cap_add")]
    pub cap_add: Vec<String>,
    #[serde(default = "default_no_new_privileges")]
    pub no_new_privileges: bool,
}

impl Default for ContainerLimits {
    fn default() -> Self {
        Self {
            memory: default_memory(),
            cpus: default_cpus(),
            pids_limit: default_pids_limit(),
            cap_drop: default_cap_drop(),
            cap_add: default_cap_add(),
            no_new_privileges: default_no_new_privileges(),
        }
    }
}

impl ContainerLimits {
    pub fn merge(&self, overrides: &ContainerLimitsOverride) -> Self {
        Self {
            memory: overrides
                .memory
                .clone()
                .unwrap_or_else(|| self.memory.clone()),
            cpus: overrides.cpus.clone().unwrap_or_else(|| self.cpus.clone()),
            pids_limit: overrides
                .pids_limit
                .clone()
                .unwrap_or_else(|| self.pids_limit.clone()),
            cap_drop: overrides
                .cap_drop
                .clone()
                .unwrap_or_else(|| self.cap_drop.clone()),
            cap_add: overrides
                .cap_add
                .clone()
                .unwrap_or_else(|| self.cap_add.clone()),
            no_new_privileges: overrides
                .no_new_privileges
                .unwrap_or(self.no_new_privileges),
        }
    }

    pub fn docker_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for cap in &self.cap_drop {
            args.push("--cap-drop".to_string());
            args.push(cap.clone());
        }
        for cap in &self.cap_add {
            args.push("--cap-add".to_string());
            args.push(cap.clone());
        }
        if self.no_new_privileges {
            args.push("--security-opt".to_string());
            args.push("no-new-privileges".to_string());
        }
        args.push("--memory".to_string());
        args.push(self.memory.clone());
        args.push("--cpus".to_string());
        args.push(self.cpus.clone());
        args.push("--pids-limit".to_string());
        args.push(self.pids_limit.clone());
        args
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ContainerLimitsOverride {
    pub memory: Option<String>,
    pub cpus: Option<String>,
    pub pids_limit: Option<String>,
    pub cap_drop: Option<Vec<String>>,
    pub cap_add: Option<Vec<String>>,
    pub no_new_privileges: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_are_secure() {
        let limits = ContainerLimits::default();
        assert_eq!(limits.cap_drop, vec!["ALL"]);
        assert_eq!(limits.cap_add, vec!["SETUID", "SETGID"]);
        assert!(limits.no_new_privileges);
        assert_eq!(limits.memory, "4g");
        assert_eq!(limits.cpus, "2");
        assert_eq!(limits.pids_limit, "256");
    }

    #[test]
    fn test_merge_overrides_specified_fields() {
        let base = ContainerLimits::default();
        let overrides = ContainerLimitsOverride {
            memory: Some("8g".to_string()),
            ..Default::default()
        };
        let merged = base.merge(&overrides);
        assert_eq!(merged.memory, "8g");
        assert_eq!(merged.cpus, "2");
        assert!(merged.no_new_privileges);
    }

    #[test]
    fn test_merge_can_loosen_security() {
        let base = ContainerLimits::default();
        let overrides = ContainerLimitsOverride {
            cap_drop: Some(vec![]),
            no_new_privileges: Some(false),
            ..Default::default()
        };
        let merged = base.merge(&overrides);
        assert!(merged.cap_drop.is_empty());
        assert!(!merged.no_new_privileges);
    }

    #[test]
    fn test_docker_args() {
        let limits = ContainerLimits::default();
        let args = limits.docker_args();
        assert!(args.contains(&"--cap-drop".to_string()));
        assert!(args.contains(&"ALL".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"SETUID".to_string()));
        assert!(args.contains(&"SETGID".to_string()));
        assert!(args.contains(&"--security-opt".to_string()));
        assert!(args.contains(&"no-new-privileges".to_string()));
        assert!(args.contains(&"--memory".to_string()));
        assert!(args.contains(&"4g".to_string()));
    }

    #[test]
    fn test_docker_args_no_security_opt_when_disabled() {
        let limits = ContainerLimits {
            no_new_privileges: false,
            cap_drop: vec![],
            ..Default::default()
        };
        let args = limits.docker_args();
        assert!(!args.contains(&"--security-opt".to_string()));
        assert!(!args.contains(&"--cap-drop".to_string()));
    }

    #[test]
    fn test_deserialize_partial() {
        let toml_str = r#"memory = "8g""#;
        let limits: ContainerLimits = toml::from_str(toml_str).unwrap();
        assert_eq!(limits.memory, "8g");
        assert_eq!(limits.cpus, "2");
        assert!(limits.no_new_privileges);
    }
}
