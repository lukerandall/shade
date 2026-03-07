pub mod macos;

use anyhow::Result;

pub trait SecretStore {
    fn set(&self, service: &str, value: &str) -> Result<()>;
    fn get(&self, service: &str) -> Result<String>;
    fn delete(&self, service: &str) -> Result<()>;
    fn list(&self, prefix: &str) -> Result<Vec<String>>;
}

/// Return the platform default secret store.
pub fn default_store() -> impl SecretStore {
    macos::MacosKeychain
}
