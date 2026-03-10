pub mod git;
pub mod jj;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Version control system.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VcsKind {
    #[default]
    Jj,
    Git,
}

/// How repos are placed in the shade directory on the host.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkMode {
    /// Symlink to the original repo (lightweight, default).
    #[default]
    Link,
    /// Independent clone (safer for untrusted agents).
    Clone,
}

/// A discovered repository.
#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    /// Display name (directory name, or group/name for nested repos).
    pub name: String,
    /// Absolute path to the repository root.
    pub path: PathBuf,
}

/// VCS operations needed by shade.
pub trait Vcs {
    /// Marker directory that identifies a repository (e.g. ".jj" or ".git").
    fn repo_marker(&self) -> &str;

    /// Human-readable name for this VCS (e.g. "jj" or "git").
    fn name(&self) -> &str;

    /// Find repositories in the given directories.
    fn discover_repos(&self, dirs: &[String]) -> Result<Vec<Repo>>;

    /// Initialize a new repository at the given path.
    fn init_repo(&self, path: &Path) -> Result<()>;

    /// Clone a repo into the target directory (independent copy).
    fn clone_repo(&self, repo: &Repo, target: &Path) -> Result<()>;

    /// Remove a workspace/worktree by name from a repo.
    #[allow(dead_code)]
    fn remove_workspace(&self, repo: &Repo, workspace_name: &str) -> Result<()>;

    /// Shell command to install this VCS tool (for Docker image builds).
    fn install_cmd(&self) -> &str;

    /// Shell command to create a workspace/worktree inside a container.
    /// `repo_path` is the container-side path to the source repo (e.g. /repos/name).
    /// `workspace_path` is the container-side path for the new workspace.
    /// `workspace_name` is the label for the workspace.
    fn container_workspace_cmd(
        &self,
        repo_path: &str,
        workspace_path: &str,
        workspace_name: &str,
    ) -> String;

    /// Shell expression that checks whether a workspace already exists at a path.
    /// Should return true (exit 0) if the workspace exists.
    fn container_workspace_exists_check(&self, workspace_path: &str) -> String;
}

/// Create a VCS implementation for the given kind.
pub fn create_vcs(kind: VcsKind) -> Box<dyn Vcs> {
    match kind {
        VcsKind::Jj => Box::new(jj::JjVcs),
        VcsKind::Git => Box::new(git::GitVcs),
    }
}

/// Discover repositories by scanning for a marker directory (e.g. ".jj" or ".git").
/// Looks one level deep for flat repos, and two levels for grouped repos (e.g. acme/core).
pub fn discover_repos_by_marker(dirs: &[String], marker: &str) -> Result<Vec<Repo>> {
    let mut repos = Vec::new();
    for dir in dirs {
        let dir_path = Path::new(dir);
        if !dir_path.is_dir() {
            continue;
        }
        let entries = std::fs::read_dir(dir_path)
            .with_context(|| format!("failed to read directory: {}", dir))?;
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.join(marker).is_dir() {
                repos.push(Repo { name, path });
            } else {
                // Scan one level deeper for grouped repos (e.g. acme/core)
                let Ok(sub_entries) = std::fs::read_dir(&path) else {
                    continue;
                };
                for sub_entry in sub_entries {
                    let Ok(sub_entry) = sub_entry else {
                        continue;
                    };
                    if !sub_entry.file_type().is_ok_and(|t| t.is_dir()) {
                        continue;
                    }
                    let sub_path = sub_entry.path();
                    if sub_path.join(marker).is_dir() {
                        let sub_name =
                            format!("{}/{}", name, sub_entry.file_name().to_string_lossy());
                        repos.push(Repo {
                            name: sub_name,
                            path: sub_path,
                        });
                    }
                }
            }
        }
    }
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(repos)
}

/// List the names of subdirectories inside `dir` that contain a VCS marker directory.
/// Checks for both `.jj` and `.git` markers. Handles both flat repos (e.g. "core")
/// and grouped repos (e.g. "acme/core").
pub fn list_repo_dirs(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.join(".jj").is_dir() || path.join(".git").is_dir() {
            result.push(name);
        } else {
            // Check one level deeper for grouped repos
            let Ok(sub_entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                if !sub_entry.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                if sub_entry.path().join(".jj").is_dir() || sub_entry.path().join(".git").is_dir() {
                    result.push(format!(
                        "{}/{}",
                        name,
                        sub_entry.file_name().to_string_lossy()
                    ));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_list_repo_dirs_jj() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("standalone/.jj")).unwrap();
        fs::create_dir_all(tmp.path().join("acme/core/.jj")).unwrap();
        fs::create_dir_all(tmp.path().join("acme/dashboard/.jj")).unwrap();
        fs::create_dir_all(tmp.path().join("acme/docs")).unwrap();
        fs::write(tmp.path().join("some-file"), "").unwrap();

        let mut dirs = list_repo_dirs(tmp.path());
        dirs.sort();

        assert_eq!(dirs.len(), 3);
        assert_eq!(dirs[0], "acme/core");
        assert_eq!(dirs[1], "acme/dashboard");
        assert_eq!(dirs[2], "standalone");
    }

    #[test]
    fn test_list_repo_dirs_git() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("my-repo/.git")).unwrap();

        let dirs = list_repo_dirs(tmp.path());
        assert_eq!(dirs, vec!["my-repo"]);
    }

    #[test]
    fn test_create_vcs_jj() {
        let vcs = create_vcs(VcsKind::Jj);
        assert_eq!(vcs.name(), "jj");
        assert_eq!(vcs.repo_marker(), ".jj");
    }

    #[test]
    fn test_create_vcs_git() {
        let vcs = create_vcs(VcsKind::Git);
        assert_eq!(vcs.name(), "git");
        assert_eq!(vcs.repo_marker(), ".git");
    }
}
