use anyhow::{Context, Result, bail};
use std::process::Command;

pub struct MacosSecretStore;

impl super::SecretStore for MacosSecretStore {
    fn set(&self, name: &str, value: &str) -> Result<()> {
        // Delete any existing entry first (ignore errors if it doesn't exist)
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", name])
            .output();

        let output = Command::new("security")
            .args(["add-generic-password", "-s", name, "-a", name, "-w", value])
            .output()
            .with_context(|| format!("failed to run security command for {name}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to store secret {name}: {stderr}");
        }

        Ok(())
    }

    fn get(&self, name: &str) -> Result<String> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", name, "-w"])
            .output()
            .with_context(|| format!("failed to read secret: {name}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("secret lookup failed for {name}: {stderr}");
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn delete(&self, name: &str) -> Result<()> {
        let output = Command::new("security")
            .args(["delete-generic-password", "-s", name])
            .output()
            .with_context(|| format!("failed to delete secret: {name}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to delete secret {name}: {stderr}");
        }

        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let output = Command::new("security")
            .args(["dump-keychain"])
            .output()
            .context("failed to list secrets")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to list secrets: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut names: Vec<String> = Vec::new();

        for line in stdout.lines() {
            // Lines look like: "svce"<blob>="shade.my-secret"
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("\"svce\"<blob>=") {
                let name = rest.trim_matches('"');
                if name.starts_with(prefix) {
                    names.push(name.to_string());
                }
            }
        }

        names.sort();
        names.dedup();
        Ok(names)
    }
}
