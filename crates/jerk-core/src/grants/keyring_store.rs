use super::{Grant, GrantError, Grants};

const SERVICE: &str = "slicc-jerk";

fn account(provider: &str, user: &str) -> String {
    format!("{provider}/{user}")
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
        let entry = keyring::Entry::new(SERVICE, &account(provider, user)).ok()?;
        let json = entry.get_password().ok()?;
        serde_json::from_str(&json).ok()
    }

    fn store(&self, grant: &Grant) -> Result<(), GrantError> {
        let json = serde_json::to_string(grant).map_err(|e| GrantError::Backend(e.to_string()))?;
        let entry = keyring::Entry::new(SERVICE, &account(&grant.provider, &grant.user))
            .map_err(|e| GrantError::Backend(e.to_string()))?;
        entry
            .set_password(&json)
            .map_err(|e| GrantError::Backend(e.to_string()))
    }

    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError> {
        let entry = keyring::Entry::new(SERVICE, &account(provider, user))
            .map_err(|e| GrantError::Backend(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(GrantError::Backend(e.to_string())),
        }
    }
}
