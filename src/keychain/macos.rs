use anyhow::{Context, Result, bail};
use std::process::Command;

pub struct MacosKeychain;

impl super::SecretStore for MacosKeychain {
    fn set(&self, service: &str, value: &str) -> Result<()> {
        // Delete any existing entry first (ignore errors if it doesn't exist)
        let _ = Command::new("security")
            .args(["delete-generic-password", "-s", service])
            .output();

        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-s",
                service,
                "-a",
                service,
                "-w",
                value,
            ])
            .output()
            .with_context(|| format!("failed to run security command for {service}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to store keychain item {service}: {stderr}");
        }

        Ok(())
    }

    fn get(&self, service: &str) -> Result<String> {
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

    fn delete(&self, service: &str) -> Result<()> {
        let output = Command::new("security")
            .args(["delete-generic-password", "-s", service])
            .output()
            .with_context(|| format!("failed to delete keychain item: {service}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to delete keychain item {service}: {stderr}");
        }

        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let output = Command::new("security")
            .args(["dump-keychain"])
            .output()
            .context("failed to dump keychain")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to list keychain items: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut services: Vec<String> = Vec::new();

        for line in stdout.lines() {
            // Lines look like: "svce"<blob>="shade.my-secret"
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("\"svce\"<blob>=") {
                let service = rest.trim_matches('"');
                if service.starts_with(prefix) {
                    services.push(service.to_string());
                }
            }
        }

        services.sort();
        services.dedup();
        Ok(services)
    }
}
