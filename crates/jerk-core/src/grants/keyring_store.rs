use super::{Grant, GrantError, Grants};
use std::sync::OnceLock;

const SERVICE: &str = "slicc-jerk";

fn account(provider: &str, user: &str) -> String {
    format!("{provider}/{user}")
}

fn init_store() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new().map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let store = windows_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let store = linux_keyutils_keyring_store::Store::new().map_err(|e| e.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

static STORE_INIT: OnceLock<Result<(), String>> = OnceLock::new();

fn ensure_store() -> Result<(), GrantError> {
    STORE_INIT
        .get_or_init(init_store)
        .as_ref()
        .map_err(|e| GrantError::Backend(e.clone()))?;
    Ok(())
}

pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        Self
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl Grants for KeyringStore {
    fn get(&self, provider: &str, user: &str) -> Option<Grant> {
        ensure_store().ok()?;
        let entry = keyring_core::Entry::new(SERVICE, &account(provider, user)).ok()?;
        let json = entry.get_password().ok()?;
        serde_json::from_str(&json).ok()
    }

    fn store(&self, grant: &Grant) -> Result<(), GrantError> {
        ensure_store()?;
        let json = serde_json::to_string(grant).map_err(|e| GrantError::Backend(e.to_string()))?;
        let entry = keyring_core::Entry::new(SERVICE, &account(&grant.provider, &grant.user))
            .map_err(|e| GrantError::Backend(e.to_string()))?;
        entry
            .set_password(&json)
            .map_err(|e| GrantError::Backend(e.to_string()))
    }

    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError> {
        ensure_store()?;
        let entry = keyring_core::Entry::new(SERVICE, &account(provider, user))
            .map_err(|e| GrantError::Backend(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(GrantError::Backend(e.to_string())),
        }
    }
}
