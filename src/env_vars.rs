use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::credentials;

/// Represents how an env var value is specified in config.
///
/// Supports three forms in TOML:
/// - `MY_VAR = "static-value"`
/// - `GH_TOKEN = { keychain = "gh-token" }`
/// - `SOME_TOKEN = { command = "cat ~/.secrets/token" }`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EnvValue {
    Static(String),
    Dynamic(EnvSource),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum EnvSource {
    Keychain(String),
    Command(String),
}

/// Resolve a map of env var definitions into concrete name=value pairs.
///
/// The `keychain_prefix` is prepended to keychain service names, so config
/// can use short names like `{ keychain = "gh-token" }` while the actual
/// keychain entry is stored as e.g. `shade.gh-token`.
pub fn resolve_env(
    env: &HashMap<String, EnvValue>,
    keychain_prefix: &str,
) -> Result<Vec<(String, String)>> {
    let mut resolved = Vec::new();

    for (key, value) in env {
        match value {
            EnvValue::Static(val) => {
                resolved.push((key.clone(), val.clone()));
            }
            EnvValue::Dynamic(EnvSource::Keychain(service)) => {
                let prefixed = format!("{keychain_prefix}{service}");
                let val = credentials::resolve_keychain(&prefixed)?;
                resolved.push((key.clone(), val));
            }
            EnvValue::Dynamic(EnvSource::Command(cmd)) => {
                let val = credentials::resolve_command(cmd)?;
                resolved.push((key.clone(), val));
            }
        }
    }

    Ok(resolved)
}

/// Merge two env maps, with overrides taking precedence.
pub fn merge_env(
    base: &HashMap<String, EnvValue>,
    overrides: &HashMap<String, EnvValue>,
) -> HashMap<String, EnvValue> {
    let mut merged = base.clone();
    for (key, value) in overrides {
        merged.insert(key.clone(), value.clone());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_static_value() {
        let toml_str = r#"MY_VAR = "hello""#;
        let parsed: HashMap<String, EnvValue> = toml::from_str(toml_str).unwrap();
        assert_eq!(
            parsed.get("MY_VAR"),
            Some(&EnvValue::Static("hello".to_string()))
        );
    }

    #[test]
    fn test_parse_keychain() {
        let toml_str = r#"GH_TOKEN = { keychain = "shade-gh-token" }"#;
        let parsed: HashMap<String, EnvValue> = toml::from_str(toml_str).unwrap();
        assert_eq!(
            parsed.get("GH_TOKEN"),
            Some(&EnvValue::Dynamic(EnvSource::Keychain(
                "shade-gh-token".to_string()
            )))
        );
    }

    #[test]
    fn test_parse_command() {
        let toml_str = r#"TOKEN = { command = "echo secret" }"#;
        let parsed: HashMap<String, EnvValue> = toml::from_str(toml_str).unwrap();
        assert_eq!(
            parsed.get("TOKEN"),
            Some(&EnvValue::Dynamic(EnvSource::Command(
                "echo secret".to_string()
            )))
        );
    }

    #[test]
    fn test_resolve_static() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), EnvValue::Static("bar".to_string()));
        let resolved = resolve_env(&env, "").unwrap();
        assert_eq!(resolved, vec![("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn test_resolve_command() {
        let mut env = HashMap::new();
        env.insert(
            "VAL".to_string(),
            EnvValue::Dynamic(EnvSource::Command("echo hello".to_string())),
        );
        let resolved = resolve_env(&env, "").unwrap();
        assert_eq!(resolved, vec![("VAL".to_string(), "hello".to_string())]);
    }

    #[test]
    fn test_merge_override() {
        let mut base = HashMap::new();
        base.insert("A".to_string(), EnvValue::Static("1".to_string()));
        base.insert("B".to_string(), EnvValue::Static("2".to_string()));

        let mut overrides = HashMap::new();
        overrides.insert("B".to_string(), EnvValue::Static("3".to_string()));
        overrides.insert("C".to_string(), EnvValue::Static("4".to_string()));

        let merged = merge_env(&base, &overrides);
        assert_eq!(merged.get("A"), Some(&EnvValue::Static("1".to_string())));
        assert_eq!(merged.get("B"), Some(&EnvValue::Static("3".to_string())));
        assert_eq!(merged.get("C"), Some(&EnvValue::Static("4".to_string())));
    }
}
