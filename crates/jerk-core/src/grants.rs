use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub provider: String,
    pub user: String,
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

pub trait Grants: Send + Sync {
    fn get(&self, provider: &str, user: &str) -> Option<Grant>;
    fn store(&self, grant: &Grant) -> Result<(), GrantError>;
    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError>;
}

#[derive(Clone, Debug, PartialEq)]
pub enum GrantError {
    Io(String),
    Backend(String),
}

impl std::fmt::Display for GrantError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantError::Io(msg) => write!(f, "grant store I/O: {msg}"),
            GrantError::Backend(msg) => write!(f, "grant store backend: {msg}"),
        }
    }
}

pub struct MemoryStore {
    entries: std::sync::Mutex<Vec<Grant>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Grants for MemoryStore {
    fn get(&self, provider: &str, user: &str) -> Option<Grant> {
        let entries = self.entries.lock().unwrap();
        entries
            .iter()
            .find(|g| g.provider == provider && g.user == user)
            .cloned()
    }

    fn store(&self, grant: &Grant) -> Result<(), GrantError> {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|g| !(g.provider == grant.provider && g.user == grant.user));
        entries.push(grant.clone());
        Ok(())
    }

    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError> {
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|g| !(g.provider == provider && g.user == user));
        Ok(())
    }
}

#[cfg(not(target_os = "wasi"))]
pub mod keyring_store;

#[cfg(not(target_os = "wasi"))]
pub mod file_store;
