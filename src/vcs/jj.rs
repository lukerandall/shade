use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Repo, Vcs, discover_repos_by_marker};

pub struct JjVcs;

impl Vcs for JjVcs {
    fn repo_marker(&self) -> &str {
        ".jj"
    }

    fn name(&self) -> &str {
        "jj"
    }

    fn discover_repos(&self, dirs: &[String]) -> Result<Vec<Repo>> {
        discover_repos_by_marker(dirs, self.repo_marker())
    }

    fn init_repo(&self, path: &Path) -> Result<()> {
        let output = Command::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .context("failed to run jj git init")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("jj git init failed: {}", stderr.trim());
        }
        Ok(())
    }

    fn clone_repo(&self, repo: &Repo, target: &Path) -> Result<()> {
        let target_path = target.join(&repo.name);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
        let output = Command::new("jj")
            .args([
                "git",
                "clone",
                &repo.path.to_string_lossy(),
                &target_path.to_string_lossy(),
            ])
            .output()
            .context("failed to run jj git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("jj git clone failed for {}: {}", repo.name, stderr.trim());
        }
        Ok(())
    }

    fn remove_workspace(&self, repo: &Repo, workspace_name: &str) -> Result<()> {
        let output = Command::new("jj")
            .args(["workspace", "forget", workspace_name])
            .current_dir(&repo.path)
            .output()
            .context("failed to run jj workspace forget")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "jj workspace forget failed for {}: {}",
                repo.name,
                stderr.trim()
            );
        }
        Ok(())
    }

    fn install_cmd(&self) -> &str {
        "cargo-binstall -y --install-path /usr/local/bin jj-cli"
    }

    fn container_workspace_cmd(
        &self,
        repo_path: &str,
        workspace_path: &str,
        workspace_name: &str,
    ) -> String {
        format!("cd {repo_path} && jj workspace add --name {workspace_name} {workspace_path}")
    }

    fn container_workspace_exists_check(&self, workspace_path: &str) -> String {
        format!("[ -d {workspace_path}/.jj ]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_repos_finds_jj_repos() {
        let tmp = TempDir::new().unwrap();
        let code_dir = tmp.path();

        std::fs::create_dir_all(code_dir.join("repo-a/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("repo-b/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("not-a-repo")).unwrap();
        std::fs::write(code_dir.join("some-file"), "").unwrap();

        let vcs = JjVcs;
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

        std::fs::create_dir_all(code_dir.join("standalone/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("acme/core/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("acme/dashboard/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("acme/docs")).unwrap();

        let vcs = JjVcs;
        let dirs = vec![code_dir.to_string_lossy().to_string()];
        let repos = vcs.discover_repos(&dirs).unwrap();

        assert_eq!(repos.len(), 3);
        assert_eq!(repos[0].name, "acme/core");
        assert_eq!(repos[1].name, "acme/dashboard");
        assert_eq!(repos[2].name, "standalone");
    }

    #[test]
    fn test_discover_repos_skips_nonexistent_dirs() {
        let vcs = JjVcs;
        let dirs = vec!["/tmp/shade-nonexistent-abc123".to_string()];
        let repos = vcs.discover_repos(&dirs).unwrap();
        assert!(repos.is_empty());
    }

    #[test]
    fn test_clone_repo_creates_independent_copy() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let target_dir = tmp.path().join("target");

        std::fs::create_dir_all(&source_dir).unwrap();
        let init = Command::new("jj")
            .args(["git", "init"])
            .current_dir(&source_dir)
            .output()
            .unwrap();
        assert!(init.status.success(), "jj git init failed");

        let repo = Repo {
            name: "my-repo".to_string(),
            path: source_dir,
        };

        let vcs = JjVcs;
        vcs.clone_repo(&repo, &target_dir).unwrap();

        let cloned = target_dir.join("my-repo");
        assert!(cloned.exists(), "clone directory should exist");
        assert!(
            cloned.join(".jj/repo").is_dir(),
            ".jj/repo should be a directory (independent clone)"
        );
    }

    #[test]
    fn test_discover_repos_multiple_dirs() {
        let tmp = TempDir::new().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");

        std::fs::create_dir_all(dir_a.join("repo-x/.jj")).unwrap();
        std::fs::create_dir_all(dir_b.join("repo-y/.jj")).unwrap();

        let vcs = JjVcs;
        let dirs = vec![
            dir_a.to_string_lossy().to_string(),
            dir_b.to_string_lossy().to_string(),
        ];
        let repos = vcs.discover_repos(&dirs).unwrap();

        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "repo-x");
        assert_eq!(repos[1].name, "repo-y");
    }

    #[test]
    fn test_init_repo() {
        let tmp = TempDir::new().unwrap();
        let vcs = JjVcs;
        vcs.init_repo(tmp.path()).unwrap();
        assert!(tmp.path().join(".jj").is_dir());
    }

    #[test]
    fn test_container_workspace_cmd() {
        let vcs = JjVcs;
        let cmd = vcs.container_workspace_cmd("/repos/core", "/workspace/core", "my-feature");
        assert_eq!(
            cmd,
            "cd /repos/core && jj workspace add --name my-feature /workspace/core"
        );
    }

    #[test]
    fn test_container_workspace_exists_check() {
        let vcs = JjVcs;
        let check = vcs.container_workspace_exists_check("/workspace/core");
        assert_eq!(check, "[ -d /workspace/core/.jj ]");
    }
}
