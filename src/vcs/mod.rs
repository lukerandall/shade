pub mod jj;

use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// How repos are linked into shades.
#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// Shared history via jj workspace (lightweight, but mutates primary repo).
    #[default]
    Workspace,
    /// Independent clone (safer for untrusted agents).
    Clone,
}

/// A discovered repository.
#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    /// Display name (directory name).
    pub name: String,
    /// Absolute path to the repository root.
    pub path: PathBuf,
}

/// VCS operations needed by shade.
#[allow(dead_code)]
pub trait Vcs {
    /// Find repositories in the given directories.
    fn discover_repos(&self, dirs: &[String]) -> Result<Vec<Repo>>;

    /// Create a linked workspace for a repo inside the target directory.
    /// `workspace_name` is used to identify the workspace (for later removal).
    fn create_workspace(&self, repo: &Repo, target: &Path, workspace_name: &str) -> Result<()>;

    /// Clone a repo into the target directory (independent copy).
    fn clone_repo(&self, repo: &Repo, target: &Path) -> Result<()>;

    /// Remove a workspace by name from a repo.
    fn remove_workspace(&self, repo: &Repo, workspace_name: &str) -> Result<()>;
}
