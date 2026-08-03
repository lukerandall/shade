use anyhow::{Context, Result};
use jiff::civil::Date;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Subdirectory of the environment directory holding archived shades (see D16).
pub const ARCHIVE_DIR: &str = "archived";

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
    list_environments_in(&PathBuf::from(env_dir))
}

/// List the archived shades under `env_dir`.
///
/// Archived shades live in the `archived/` subdirectory of the environment
/// directory (see D16). That name doesn't parse as `YYYY-MM-DD-label`, so
/// `list_environments` skips it and the archive stays invisible to `shade list`,
/// the TUI, and anything else enumerating active shades.
pub fn list_archived(env_dir: &str) -> Result<Vec<Environment>> {
    list_environments_in(&archive_dir(env_dir))
}

/// Path of the archive directory for an environment directory.
pub fn archive_dir(env_dir: &str) -> PathBuf {
    PathBuf::from(env_dir).join(ARCHIVE_DIR)
}

/// Scan one directory for validly-named shade directories.
fn list_environments_in(dir_path: &Path) -> Result<Vec<Environment>> {
    if !dir_path.exists() {
        return Ok(Vec::new());
    }

    let entries = std::fs::read_dir(dir_path).with_context(|| {
        format!(
            "failed to read environment directory: {}",
            dir_path.display()
        )
    })?;

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

/// Move an active shade into the archive, returning it relocated.
///
/// The directory is moved wholesale to `env_dir/archived/<name>`; nothing inside
/// it is touched. Errors if the shade is missing or already archived.
pub fn archive_environment(env: &Environment, env_dir: &str) -> Result<Environment> {
    move_environment(env, &archive_dir(env_dir))
}

/// Move an archived shade back to the active environment directory.
pub fn unarchive_environment(env: &Environment, env_dir: &str) -> Result<Environment> {
    move_environment(env, &PathBuf::from(env_dir))
}

/// Relocate a shade directory into `target_dir`, keeping its name.
fn move_environment(env: &Environment, target_dir: &Path) -> Result<Environment> {
    if !env.path.exists() {
        return Err(EnvError::NotFound(env.name.clone()).into());
    }

    let target = target_dir.join(&env.name);
    if target.exists() {
        return Err(EnvError::AlreadyExists(target.display().to_string()).into());
    }

    std::fs::create_dir_all(target_dir).with_context(|| {
        format!(
            "failed to create destination directory: {}",
            target_dir.display()
        )
    })?;

    std::fs::rename(&env.path, &target).with_context(|| {
        format!(
            "failed to move {} to {}",
            env.path.display(),
            target.display()
        )
    })?;

    Ok(Environment {
        path: target,
        ..env.clone()
    })
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
    fn test_archive_moves_directory_into_archive_subdir() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "finished").unwrap();
        fs::write(env.path.join("TASK.md"), "the brief").unwrap();

        let archived = archive_environment(&env, env_dir).unwrap();

        assert!(!env.path.exists());
        assert_eq!(archived.path, tmp.path().join(ARCHIVE_DIR).join(&env.name));
        assert!(archived.path.is_dir());
        // The record files move with it, untouched.
        assert_eq!(
            fs::read_to_string(archived.path.join("TASK.md")).unwrap(),
            "the brief"
        );
        // Name, label, and date are preserved.
        assert_eq!(archived.name, env.name);
        assert_eq!(archived.label, "finished");
        assert_eq!(archived.date, env.date);
    }

    #[test]
    fn test_archived_shades_are_hidden_from_list_and_shown_by_list_archived() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let kept = create_environment(env_dir, "active").unwrap();
        let done = create_environment(env_dir, "retired").unwrap();
        archive_environment(&done, env_dir).unwrap();

        let active = list_environments(env_dir).unwrap();
        assert_eq!(active.len(), 1, "archive dir must not appear in shade list");
        assert_eq!(active[0].name, kept.name);

        let archived = list_archived(env_dir).unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].name, done.name);
        assert_eq!(archived[0].label, "retired");
    }

    #[test]
    fn test_list_archived_when_no_archive_exists() {
        let tmp = TempDir::new().unwrap();
        let archived = list_archived(tmp.path().to_str().unwrap()).unwrap();
        assert!(archived.is_empty());
    }

    #[test]
    fn test_archive_then_unarchive_round_trips() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "comeback").unwrap();
        fs::write(env.path.join("LOG.md"), "history").unwrap();
        let original_path = env.path.clone();

        let archived = archive_environment(&env, env_dir).unwrap();
        let restored = unarchive_environment(&archived, env_dir).unwrap();

        assert_eq!(restored.path, original_path);
        assert!(restored.path.is_dir());
        assert!(!archived.path.exists());
        assert_eq!(
            fs::read_to_string(restored.path.join("LOG.md")).unwrap(),
            "history"
        );
        // It is an ordinary active shade again.
        let active = list_environments(env_dir).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, env.name);
        assert!(list_archived(env_dir).unwrap().is_empty());
    }

    #[test]
    fn test_archive_refuses_when_already_present_in_archive() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "twice").unwrap();
        archive_environment(&env, env_dir).unwrap();
        // Recreate a shade with the same name, then try to archive it again.
        let again = create_environment(env_dir, "twice").unwrap();

        let err = archive_environment(&again, env_dir).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        // The would-be source is left alone rather than half-moved.
        assert!(again.path.exists());
    }

    #[test]
    fn test_unarchive_refuses_when_active_shade_of_same_name_exists() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();

        let env = create_environment(env_dir, "clash").unwrap();
        let archived = archive_environment(&env, env_dir).unwrap();
        create_environment(env_dir, "clash").unwrap();

        let err = unarchive_environment(&archived, env_dir).unwrap_err();
        assert!(err.to_string().contains("already exists"));
        assert!(archived.path.exists());
    }

    #[test]
    fn test_archive_nonexistent_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let env_dir = tmp.path().to_str().unwrap();
        let ghost = Environment {
            name: "2026-03-05-ghost".to_string(),
            label: "ghost".to_string(),
            date: "2026-03-05".parse().unwrap(),
            path: tmp.path().join("2026-03-05-ghost"),
        };

        let err = archive_environment(&ghost, env_dir).unwrap_err();
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
