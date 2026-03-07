use anyhow::{Context, Result, bail};
use std::process::Command;

/// Resolve a `{ command = "..." }` style env var.
pub fn resolve_command(command: &str) -> Result<String> {
    let output = Command::new("sh")
        .args(["-c", command])
        .output()
        .with_context(|| format!("failed to run command: {command}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("command failed: {command}: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Resolve a `{ keychain = "service-name" }` style env var via macOS Keychain.
pub fn resolve_keychain(service: &str) -> Result<String> {
    let output = Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .with_context(|| format!("failed to read keychain item: {service}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("keychain lookup failed for {service}: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_command() {
        let result = resolve_command("echo hello").unwrap();
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_resolve_command_failure() {
        let result = resolve_command("false");
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_keychain_missing_item() {
        let result = resolve_keychain("shade-test-nonexistent-item-abc123");
        assert!(result.is_err());
    }
}
