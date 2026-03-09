use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{Repo, Vcs};

pub struct JjVcs;

impl Vcs for JjVcs {
    fn discover_repos(&self, dirs: &[String]) -> Result<Vec<Repo>> {
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
                if path.join(".jj").is_dir() {
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
                        if sub_path.join(".jj").is_dir() {
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

    fn create_workspace(&self, repo: &Repo, target: &Path, workspace_name: &str) -> Result<()> {
        let target_path = target.join(&repo.name);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory: {}", parent.display()))?;
        }
        let output = Command::new("jj")
            .args([
                "workspace",
                "add",
                "--name",
                workspace_name,
                &target_path.to_string_lossy(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discover_repos_finds_jj_repos() {
        let tmp = TempDir::new().unwrap();
        let code_dir = tmp.path();

        // Create some fake repos
        std::fs::create_dir_all(code_dir.join("repo-a/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("repo-b/.jj")).unwrap();
        // Not a jj repo
        std::fs::create_dir_all(code_dir.join("not-a-repo")).unwrap();
        // A file, not a directory
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

        // Top-level repo
        std::fs::create_dir_all(code_dir.join("standalone/.jj")).unwrap();
        // Grouped repos under a subdirectory
        std::fs::create_dir_all(code_dir.join("acme/core/.jj")).unwrap();
        std::fs::create_dir_all(code_dir.join("acme/dashboard/.jj")).unwrap();
        // Non-repo subdirectory inside a group
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

        // Create a real jj repo as the source
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
        // A clone's .jj/repo is a directory, not a file
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
        // Sorted alphabetically
        assert_eq!(repos[0].name, "repo-x");
        assert_eq!(repos[1].name, "repo-y");
    }
}
