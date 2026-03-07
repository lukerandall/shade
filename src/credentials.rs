use anyhow::{Context, Result, bail};
use std::process::Command;

struct Provider {
    env_var: &'static str,
    command: &'static [&'static str],
}

const PROVIDERS: &[(&str, Provider)] = &[(
    "gh",
    Provider {
        env_var: "GH_TOKEN",
        command: &["gh", "auth", "token"],
    },
)];

#[derive(Debug)]
pub struct ResolvedEnv {
    pub name: String,
    pub value: String,
}

/// Look up a known provider by name and resolve its value.
pub fn resolve_provider(name: &str) -> Result<ResolvedEnv> {
    let (_, provider) = PROVIDERS
        .iter()
        .find(|(n, _)| *n == name)
        .with_context(|| format!("unknown credential provider: {name}"))?;

    let output = Command::new(provider.command[0])
        .args(&provider.command[1..])
        .output()
        .with_context(|| format!("failed to run credential command for {name}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("credential command for {name} failed: {stderr}");
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();

    Ok(ResolvedEnv {
        name: provider.env_var.to_string(),
        value,
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_gh_provider_exists() {
        // Just verify the provider is registered, not that gh is authed
        let (_, provider) = PROVIDERS.iter().find(|(n, _)| *n == "gh").unwrap();
        assert_eq!(provider.env_var, "GH_TOKEN");
    }

    #[test]
    fn test_unknown_provider_returns_error() {
        let result = resolve_provider("nonexistent");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown"));
    }
}
