use super::{Grant, GrantError, Grants};
use std::fs;
use std::path::PathBuf;

pub struct FileStore {
    dir: PathBuf,
}

impl FileStore {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            dir: state_dir.join("grants"),
        }
    }

    fn path(&self, provider: &str, user: &str) -> PathBuf {
        let key = format!("{provider}\0{user}");
        let hash = hex_encode(&simple_hash(key.as_bytes()));
        self.dir.join(format!("{hash}.json"))
    }
}

fn simple_hash(data: &[u8]) -> [u8; 16] {
    let mut h: u128 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u128;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h.to_le_bytes()
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    provider: String,
    user: String,
    grant: Grant,
}

impl Grants for FileStore {
    fn get(&self, provider: &str, user: &str) -> Option<Grant> {
        let path = self.path(provider, user);
        let data = fs::read_to_string(path).ok()?;
        let envelope: Envelope = serde_json::from_str(&data).ok()?;
        if envelope.provider != provider || envelope.user != user {
            return None;
        }
        Some(envelope.grant)
    }

    fn store(&self, grant: &Grant) -> Result<(), GrantError> {
        fs::create_dir_all(&self.dir).map_err(|e| GrantError::Io(e.to_string()))?;
        let path = self.path(&grant.provider, &grant.user);
        let envelope = Envelope {
            provider: grant.provider.clone(),
            user: grant.user.clone(),
            grant: grant.clone(),
        };
        let json =
            serde_json::to_string(&envelope).map_err(|e| GrantError::Backend(e.to_string()))?;
        fs::write(&path, &json).map_err(|e| GrantError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(&path, perms).map_err(|e| GrantError::Io(e.to_string()))?;
        }
        Ok(())
    }

    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError> {
        let path = self.path(provider, user);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(GrantError::Io(e.to_string())),
        }
    }
}
