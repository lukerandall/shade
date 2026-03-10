use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Repo, Vcs, discover_repos_by_marker};

pub struct GitVcs;

impl Vcs for GitVcs {
    fn repo_marker(&self) -> &str {
        ".git"
    }

    fn name(&self) -> &str {
        "git"
    }

    fn discover_repos(&self, dirs: &[String]) -> Result<Vec<Repo>> {
        discover_repos_by_marker(dirs, self.repo_marker())
    }

    fn init_repo(&self, path: &Path) -> Result<()> {
        let output = Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .context("failed to run git init")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git init failed: {}", stderr.trim());
        }
        Ok(())
    }

    fn clone_repo(&self, repo: &Repo, target: &Path) -> Result<()> {
        let target_path = target.join(&repo.name);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
        let output = Command::new("git")
            .args([
                "clone",
                &repo.path.to_string_lossy(),
                &target_path.to_string_lossy(),
            ])
            .output()
            .context("failed to run git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git clone failed for {}: {}", repo.name, stderr.trim());
        }
        Ok(())
    }

    fn remove_workspace(&self, repo: &Repo, workspace_name: &str) -> Result<()> {
        let output = Command::new("git")
            .args(["worktree", "remove", workspace_name, "--force"])
            .current_dir(&repo.path)
            .output()
            .context("failed to run git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git worktree remove failed for {}: {}",
                repo.name,
                stderr.trim()
            );
        }
        Ok(())
    }

    fn install_cmd(&self) -> &str {
        "apt-get update -qq && apt-get install -y -qq git >/dev/null"
    }

    fn container_workspace_cmd(
        &self,
        repo_path: &str,
        workspace_path: &str,
        workspace_name: &str,
    ) -> String {
        format!("cd {repo_path} && git worktree add -b {workspace_name} {workspace_path}")
    }

    fn container_workspace_exists_check(&self, workspace_path: &str) -> String {
        format!("[ -d {workspace_path}/.git ]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_repos_finds_git_repos() {
        let tmp = TempDir::new().unwrap();
        let code_dir = tmp.path();

        std::fs::create_dir_all(code_dir.join("repo-a/.git")).unwrap();
        std::fs::create_dir_all(code_dir.join("repo-b/.git")).unwrap();
        std::fs::create_dir_all(code_dir.join("not-a-repo")).unwrap();

        let vcs = GitVcs;
        let dirs = vec![code_dir.to_string_lossy().to_string()];
        let repos = vcs.discover_repos(&dirs).unwrap();

        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "repo-a");
        assert_eq!(repos[1].name, "repo-b");
    }

    #[test]
    fn test_discover_repos_finds_nested_repos() {
        let tmp = TempDir::new().unwrap();
        let code_dir = tmp.path();

        std::fs::create_dir_all(code_dir.join("standalone/.git")).unwrap();
        std::fs::create_dir_all(code_dir.join("acme/core/.git")).unwrap();

        let vcs = GitVcs;
        let dirs = vec![code_dir.to_string_lossy().to_string()];
        let repos = vcs.discover_repos(&dirs).unwrap();

        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "acme/core");
        assert_eq!(repos[1].name, "standalone");
    }

    #[test]
    fn test_init_repo() {
        let tmp = TempDir::new().unwrap();
        let vcs = GitVcs;
        vcs.init_repo(tmp.path()).unwrap();
        assert!(tmp.path().join(".git").is_dir());
    }

    #[test]
    fn test_container_workspace_cmd() {
        let vcs = GitVcs;
        let cmd = vcs.container_workspace_cmd("/repos/core", "/workspace/core", "my-feature");
        assert_eq!(
            cmd,
            "cd /repos/core && git worktree add -b my-feature /workspace/core"
        );
    }

    #[test]
    fn test_container_workspace_exists_check() {
        let vcs = GitVcs;
        let check = vcs.container_workspace_exists_check("/workspace/core");
        assert_eq!(check, "[ -d /workspace/core/.git ]");
    }
}
