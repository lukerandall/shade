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

    fn add_workspace(
        &self,
        repo: &Repo,
        workspace_path: &Path,
        workspace_name: &str,
    ) -> Result<()> {
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                workspace_name,
                &workspace_path.to_string_lossy(),
            ])
            .current_dir(&repo.path)
            .output()
            .context("failed to run git worktree add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git worktree add failed for {}: {}",
                repo.name,
                stderr.trim()
            );
        }
        Ok(())
    }

    fn remove_workspace(
        &self,
        source_repo: &Path,
        workspace_name: &str,
        workspace_path: &Path,
    ) -> Result<()> {
        // Best-effort: the source repo may have been moved or deleted.
        if !source_repo.exists() {
            return Ok(());
        }
        let output = Command::new("git")
            .args([
                "worktree",
                "remove",
                "--force",
                &workspace_path.to_string_lossy(),
            ])
            .current_dir(source_repo)
            .output()
            .context("failed to run git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "git worktree remove failed for {}: {}",
                workspace_path.display(),
                stderr.trim()
            );
        }

        // The `-b` from add_workspace leaves a branch behind. Best-effort delete;
        // do not fail the whole teardown if the branch is already gone.
        let _ = Command::new("git")
            .args(["branch", "-D", workspace_name])
            .current_dir(source_repo)
            .output();

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

    fn init_git_repo_with_commit(path: &std::path::Path) {
        std::fs::create_dir_all(path).unwrap();
        let run = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(path)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
        };
        run(&["init"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        std::fs::write(path.join("README.md"), "hello").unwrap();
        run(&["add", "."]);
        run(&["commit", "-m", "initial"]);
    }

    fn git_worktree_list(source_repo: &std::path::Path) -> String {
        let out = Command::new("git")
            .args(["worktree", "list"])
            .current_dir(source_repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn test_add_workspace_creates_worktree_with_branch() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_git_repo_with_commit(&source);
        let ws_path = tmp.path().join("ws");
        let repo = Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };

        let vcs = GitVcs;
        vcs.add_workspace(&repo, &ws_path, "my-shade").unwrap();

        assert!(
            ws_path.join(".git").exists(),
            ".git should exist in worktree"
        );
        let listed = git_worktree_list(&source);
        assert!(
            listed.contains("my-shade"),
            "worktree should be on branch my-shade: {listed}"
        );
    }

    #[test]
    fn test_remove_workspace_removes_worktree_and_branch() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_git_repo_with_commit(&source);
        let ws_path = tmp.path().join("ws");
        let repo = Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };

        let vcs = GitVcs;
        vcs.add_workspace(&repo, &ws_path, "my-shade").unwrap();
        assert!(ws_path.exists());

        vcs.remove_workspace(&source, "my-shade", &ws_path).unwrap();
        assert!(!ws_path.exists(), "worktree dir should be gone");
        let branches = Command::new("git")
            .args(["branch", "--list", "my-shade"])
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&branches.stdout).trim().is_empty(),
            "branch my-shade should be deleted"
        );
    }

    #[test]
    fn test_remove_workspace_ok_when_source_absent() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let ws_path = tmp.path().join("ws");
        let vcs = GitVcs;
        // Must not panic or error when the source repo is gone.
        vcs.remove_workspace(&missing, "my-shade", &ws_path)
            .unwrap();
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
