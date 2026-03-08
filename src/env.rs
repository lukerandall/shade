use anyhow::{Context, Result};
use jiff::civil::Date;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnvError {
    #[error("environment already exists: {0}")]
    AlreadyExists(String),

    #[error("environment does not exist: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Environment {
    /// Full directory name, e.g. "2026-03-05-my-feature"
    pub name: String,
    /// Label portion without date prefix, e.g. "my-feature"
    pub label: String,
    /// Parsed date from the directory name
    pub date: Date,
    /// Full path to the environment directory
    pub path: PathBuf,
}

/// Parse a directory name like "2026-03-05-my-feature" into (Date, label).
/// Returns None if the name doesn't match the expected pattern.
fn parse_env_name(name: &str) -> Option<(Date, String)> {
    // Need at least "YYYY-MM-DD-x" = 11 characters
    if name.len() < 11 {
        return None;
    }

    // Check that position 10 is a dash (separator between date and label)
    if name.as_bytes().get(10) != Some(&b'-') {
        return None;
    }

    let date_str = &name[..10];
    let label = &name[11..];

    if label.is_empty() {
        return None;
    }

    let date: Date = date_str.parse().ok()?;
    Some((date, label.to_string()))
}

/// List all valid shade environments in the given directory.
///
/// Returns environments sorted by date descending (newest first), then by name.
/// Silently skips directories that don't match the expected naming pattern.
/// Returns an empty vec if the directory doesn't exist.
pub fn list_environments(env_dir: &str) -> Result<Vec<Environment>> {
    let dir_path = PathBuf::from(env_dir);

    if !dir_path.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(&dir_path)
        .with_context(|| format!("failed to read environment directory: {}", env_dir))?;

    let mut envs: Vec<Environment> = Vec::new();

    for entry in entries {
        let entry = entry.context("failed to read directory entry")?;

        // Only consider directories
        let file_type = entry.file_type().context("failed to get file type")?;
        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        if let Some((date, label)) = parse_env_name(&name) {
            envs.push(Environment {
                name: name.clone(),
                label,
                date,
                path: dir_path.join(&name),
            });
        }
    }

    // Sort by date descending, then by name ascending for ties
    envs.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.name.cmp(&b.name)));

    Ok(envs)
}

/// Create a new shade environment with today's date and the given label.
///
/// The label should already be slugified. Creates env_dir if it doesn't exist.
pub fn create_environment(env_dir: &str, label: &str) -> Result<Environment> {
    let today = jiff::Zoned::now().date();
    let name = format!("{}-{}", today, label);
    let dir_path = PathBuf::from(env_dir);
    let env_path = dir_path.join(&name);

    if env_path.exists() {
        return Err(EnvError::AlreadyExists(name).into());
    }

    std::fs::create_dir_all(&env_path).with_context(|| {
        format!(
            "failed to create environment directory: {}",
            env_path.display()
        )
    })?;

    Ok(Environment {
        name,
        label: label.to_string(),
        date: today,
        path: env_path,
    })
}

/// List the names of subdirectories inside `dir` that contain a `.jj` directory.
///
/// Used to discover which repos have workspaces checked out inside a shade.
pub fn list_workspace_dirs(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| e.path().join(".jj").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect()
}

/// Delete an environment by removing its directory recursively.
pub fn delete_environment(env: &Environment) -> Result<()> {
    if !env.path.exists() {
        return Err(EnvError::NotFound(env.name.clone()).into());
    }

    std::fs::remove_dir_all(&env.path)
        .with_context(|| format!("failed to delete environment: {}", env.path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_list_empty_directory() {
        let tmp = TempDir::new().unwrap();
        let envs = list_environments(tmp.path().to_str().unwrap()).unwrap();
        assert!(envs.is_empty());
    }

    #[test]
    fn test_list_nonexistent_directory() {
        let envs = list_environments("/tmp/shade-test-nonexistent-dir-abc123").unwrap();
        assert!(envs.is_empty());
    }

    #[test]
    fn test_list_finds_and_parses_valid_environments() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        fs::create_dir(tmp.path().join("2026-03-05-my-feature")).unwrap();
        fs::create_dir(tmp.path().join("2026-02-28-other-thing")).unwrap();

        let envs = list_environments(env_dir).unwrap();
        assert_eq!(envs.len(), 2);

        assert_eq!(envs[0].name, "2026-03-05-my-feature");
        assert_eq!(envs[0].label, "my-feature");
        assert_eq!(envs[0].date, "2026-03-05".parse::<Date>().unwrap());

        assert_eq!(envs[1].name, "2026-02-28-other-thing");
        assert_eq!(envs[1].label, "other-thing");
    }

    #[test]
    fn test_list_ignores_non_matching_directories() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        // Valid
        fs::create_dir(tmp.path().join("2026-03-05-valid")).unwrap();
        // Invalid patterns
        fs::create_dir(tmp.path().join("not-a-date-dir")).unwrap();
        fs::create_dir(tmp.path().join("2026-03-05")).unwrap(); // no label
        fs::create_dir(tmp.path().join("abcd-ef-gh-nope")).unwrap();
        // File, not directory
        fs::write(tmp.path().join("2026-03-05-a-file"), "").unwrap();

        let envs = list_environments(env_dir).unwrap();
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].name, "2026-03-05-valid");
    }

    #[test]
    fn test_list_sorts_by_date_descending() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        fs::create_dir(tmp.path().join("2026-01-01-oldest")).unwrap();
        fs::create_dir(tmp.path().join("2026-06-15-middle")).unwrap();
        fs::create_dir(tmp.path().join("2026-12-31-newest")).unwrap();
        // Same date, different names — should sort alphabetically
        fs::create_dir(tmp.path().join("2026-06-15-alpha")).unwrap();

        let envs = list_environments(env_dir).unwrap();
        assert_eq!(envs.len(), 4);
        assert_eq!(envs[0].name, "2026-12-31-newest");
        assert_eq!(envs[1].name, "2026-06-15-alpha");
        assert_eq!(envs[2].name, "2026-06-15-middle");
        assert_eq!(envs[3].name, "2026-01-01-oldest");
    }

    #[test]
    fn test_create_makes_directory_and_returns_environment() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "my-project").unwrap();

        assert!(env.path.exists());
        assert!(env.path.is_dir());
        assert_eq!(env.label, "my-project");
        assert_eq!(env.date, jiff::Zoned::now().date());

        let today = jiff::Zoned::now().date();
        let expected_name = format!("{}-my-project", today);
        assert_eq!(env.name, expected_name);
    }

    #[test]
    fn test_create_with_nonexistent_env_dir() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().join("nested").join("envs");
        let env_dir_str = env_dir.to_str().unwrap();

        let env = create_environment(env_dir_str, "test-env").unwrap();
        assert!(env.path.exists());
        assert!(env.path.is_dir());
    }

    #[test]
    fn test_create_duplicate_returns_error() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        create_environment(env_dir, "duplicate").unwrap();
        let result = create_environment(env_dir, "duplicate");

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_delete_removes_directory() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "to-delete").unwrap();
        assert!(env.path.exists());

        delete_environment(&env).unwrap();
        assert!(!env.path.exists());
    }

    #[test]
    fn test_delete_nonexistent_returns_error() {
        let env = Environment {
            name: "2026-03-05-ghost".to_string(),
            label: "ghost".to_string(),
            date: "2026-03-05".parse().unwrap(),
            path: PathBuf::from("/tmp/shade-nonexistent-abc123/2026-03-05-ghost"),
        };

        let result = delete_environment(&env);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("does not exist"));
    }

    #[test]
    fn test_parse_env_name_valid() {
        let (date, label) = parse_env_name("2026-03-05-my-feature").unwrap();
        assert_eq!(date, "2026-03-05".parse::<Date>().unwrap());
        assert_eq!(label, "my-feature");
    }

    #[test]
    fn test_parse_env_name_invalid() {
        assert!(parse_env_name("not-valid").is_none());
        assert!(parse_env_name("2026-03-05").is_none()); // no label
        assert!(parse_env_name("").is_none());
        assert!(parse_env_name("abcd-ef-gh-label").is_none());
    }
}
