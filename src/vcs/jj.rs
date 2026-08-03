use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Repo, Vcs, discover_repos_by_marker};

pub struct JjVcs;

/// Summarise the "Working copy changes:" block of `jj status` output.
///
/// The block runs from that header until the first line that isn't a change entry
/// (`M path`, `A path`, `D path`, …) — in practice the blank line or the
/// `Working copy  (@) :` / `Parent commit (@-):` summary that follows it. Returns
/// one line per changed path, or empty when the working copy is clean.
fn describe_working_copy_changes(status_stdout: &str) -> Vec<String> {
    let mut changes = Vec::new();
    let mut in_block = false;
    for line in status_stdout.lines() {
        if line.starts_with("Working copy changes:") {
            in_block = true;
            continue;
        }
        if !in_block {
            continue;
        }
        // Change entries are indented or start with a single-letter status code
        // followed by a space; anything else ends the block.
        let entry = line.trim_end();
        let is_change = entry
            .split_once(' ')
            .is_some_and(|(code, path)| code.len() == 1 && !path.trim().is_empty());
        if is_change {
            changes.push(format!("uncommitted change: {}", entry.trim()));
        } else {
            break;
        }
    }
    changes
}

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

    fn add_workspace(
        &self,
        repo: &Repo,
        workspace_path: &Path,
        workspace_name: &str,
    ) -> Result<()> {
        let output = Command::new("jj")
            .args([
                "workspace",
                "add",
                "--name",
                workspace_name,
                &workspace_path.to_string_lossy(),
            ])
            .current_dir(&repo.path)
            .output()
            .context("failed to run jj workspace add")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "jj workspace add failed for {}: {}",
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
        _workspace_path: &Path,
    ) -> Result<()> {
        // Best-effort: the source repo may have been moved or deleted.
        if !source_repo.exists() {
            return Ok(());
        }
        let output = Command::new("jj")
            .args(["workspace", "forget", workspace_name])
            .current_dir(source_repo)
            .output()
            .context("failed to run jj workspace forget")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "jj workspace forget failed for {}: {}",
                workspace_name,
                stderr.trim()
            );
        }
        Ok(())
    }

    /// Only *uncommitted* working-copy changes are at risk in jj. Running `jj
    /// status` snapshots the working copy into the `@` commit, and that commit
    /// lives in the source repo's store — so `jj workspace forget` followed by
    /// deleting the directory loses no committed work, bookmarked or not. What it
    /// does lose is anything jj isn't tracking (ignored files, build output, a
    /// local `.env`), which is why a dirty working copy is reported.
    fn workspace_work_at_risk(&self, workspace_path: &Path) -> Result<Vec<String>> {
        if !workspace_path.exists() {
            return Ok(Vec::new());
        }
        let output = Command::new("jj")
            .args(["status"])
            .current_dir(workspace_path)
            .output()
            .context("failed to run jj status")?;
        if !output.status.success() {
            // Not a jj workspace (or jj can't read it) — nothing we can assert.
            return Ok(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(describe_working_copy_changes(&stdout))
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

    fn init_jj_repo(path: &std::path::Path) {
        std::fs::create_dir_all(path).unwrap();
        let init = Command::new("jj")
            .args(["git", "init"])
            .current_dir(path)
            .output()
            .unwrap();
        assert!(init.status.success(), "jj git init failed");
    }

    fn jj_workspace_list(source_repo: &std::path::Path) -> String {
        let out = Command::new("jj")
            .args(["workspace", "list"])
            .current_dir(source_repo)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    #[test]
    fn test_add_workspace_creates_and_registers() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let ws_path = tmp.path().join("ws");
        let repo = Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };

        let vcs = JjVcs;
        vcs.add_workspace(&repo, &ws_path, "my-shade").unwrap();

        assert!(
            ws_path.join(".jj").is_dir(),
            ".jj should exist in workspace"
        );
        assert!(
            jj_workspace_list(&source).contains("my-shade"),
            "workspace should be registered in the source repo"
        );
    }

    #[test]
    fn test_remove_workspace_forgets_registration() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let ws_path = tmp.path().join("ws");
        let repo = Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };

        let vcs = JjVcs;
        vcs.add_workspace(&repo, &ws_path, "my-shade").unwrap();
        assert!(jj_workspace_list(&source).contains("my-shade"));

        vcs.remove_workspace(&source, "my-shade", &ws_path).unwrap();
        assert!(
            !jj_workspace_list(&source).contains("my-shade"),
            "workspace should be forgotten after removal"
        );
    }

    #[test]
    fn test_remove_workspace_ok_when_source_absent() {
        let tmp = TempDir::new().unwrap();
        let missing = tmp.path().join("does-not-exist");
        let ws_path = tmp.path().join("ws");
        let vcs = JjVcs;
        // Must not panic or error when the source repo is gone.
        vcs.remove_workspace(&missing, "my-shade", &ws_path)
            .unwrap();
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

    #[test]
    fn test_work_at_risk_flags_a_dirty_workspace_and_clears_when_clean() {
        let tmp = TempDir::new().unwrap();
        let source = tmp.path().join("source");
        init_jj_repo(&source);
        let ws_path = tmp.path().join("ws");
        let repo = Repo {
            name: "my-repo".to_string(),
            path: source.clone(),
        };
        let vcs = JjVcs;
        vcs.add_workspace(&repo, &ws_path, "my-shade").unwrap();

        assert!(
            vcs.workspace_work_at_risk(&ws_path).unwrap().is_empty(),
            "a fresh workspace has an empty working copy"
        );

        std::fs::write(ws_path.join("scratch.txt"), "work in progress").unwrap();
        let dirty = vcs.workspace_work_at_risk(&ws_path).unwrap();
        assert!(
            dirty.iter().any(|l| l.contains("scratch.txt")),
            "new file should be reported as at risk: {dirty:?}"
        );
    }

    #[test]
    fn test_work_at_risk_empty_for_missing_path() {
        let tmp = TempDir::new().unwrap();
        let vcs = JjVcs;
        assert!(
            vcs.workspace_work_at_risk(&tmp.path().join("nope"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_working_copy_changes_reports_each_changed_path() {
        let status = "Working copy changes:\nM src/main.rs\nA notes.txt\nD old.rs\nWorking copy  (@) : kntqzsrp 0f1e2d3c (no description set)\nParent commit (@-): rlvkpnrz 4a5b6c7d main | Add thing\n";

        let at_risk = describe_working_copy_changes(status);

        assert_eq!(
            at_risk,
            vec![
                "uncommitted change: M src/main.rs",
                "uncommitted change: A notes.txt",
                "uncommitted change: D old.rs",
            ]
        );
    }

    #[test]
    fn test_working_copy_changes_empty_when_clean() {
        let status = "The working copy has no changes.\nWorking copy  (@) : kntqzsrp 0f1e2d3c (empty) (no description set)\nParent commit (@-): rlvkpnrz 4a5b6c7d main | Add thing\n";

        assert!(describe_working_copy_changes(status).is_empty());
    }

    #[test]
    fn test_working_copy_changes_stops_at_end_of_block() {
        // A blank line ends the change block; later prose must not be picked up.
        let status =
            "Working copy changes:\nM src/main.rs\n\nHint: use `jj new` to start a new change.\n";

        assert_eq!(
            describe_working_copy_changes(status),
            vec!["uncommitted change: M src/main.rs"]
        );
    }
}
