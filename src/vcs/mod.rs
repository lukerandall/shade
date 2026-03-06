pub mod jj;

use anyhow::Result;
use std::path::{Path, PathBuf};

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

    /// Remove a workspace by name from a repo.
    fn remove_workspace(&self, repo: &Repo, workspace_name: &str) -> Result<()>;
}
