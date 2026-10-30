use crate::paths::Env;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const NOT_PRIVATE: &str = "the state directory is not private";

pub struct ParamFile(PathBuf);

impl ParamFile {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ParamFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    let mine = meta.uid() == unsafe { libc::geteuid() };
    if meta.is_dir() && mine && meta.mode() & 0o022 == 0 {
        return Ok(());
    }
    Err(std::io::Error::other(NOT_PRIVATE))
}

#[cfg(not(unix))]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

pub fn param_file(env: &Env, id: &str, text: &str) -> std::io::Result<ParamFile> {
    let dir: PathBuf = env.state_dir().components().collect();
    private_dir(&dir)?;
    let path = dir.join(format!("param-{id}"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path)?;
    let param = ParamFile(path);
    file.write_all(text.as_bytes())?;
    Ok(param)
}

fn file(env: &Env) -> PathBuf {
    env.state_dir().join("minted.json")
}

fn leases_file(env: &Env) -> PathBuf {
    env.state_dir().join("leases.json")
}

fn read(env: &Env) -> Value {
    read_at(file(env))
}

fn read_at(path: PathBuf) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn write(env: &Env, doc: &Value) -> std::io::Result<()> {
    write_at(env, file(env), doc)
}

fn write_at(env: &Env, path: PathBuf, doc: &Value) -> std::io::Result<()> {
    std::fs::create_dir_all(env.state_dir())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, format!("{doc}\n"))?;
    restrict(&tmp)?;
    std::fs::rename(tmp, path)
}

#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn minted(env: &Env, provider: &str) -> Vec<String> {
    read(env)[provider]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn record(env: &Env, provider: &str, id: &str) -> std::io::Result<()> {
    let mut doc = read(env);
    let mut ids = minted(env, provider);
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_string());
    }
    doc[provider] = json!(ids);
    write(env, &doc)
}

pub fn forget(env: &Env, provider: &str, id: &str) -> std::io::Result<bool> {
    let mut ids = minted(env, provider);
    let before = ids.len();
    ids.retain(|i| i != id);
    if ids.len() == before {
        return Ok(false);
    }
    let mut doc = read(env);
    doc[provider] = json!(ids);
    write(env, &doc).map(|_| true)
}

pub fn lease_record(env: &Env, lease_id: &str, provider: &str, id: &str) -> std::io::Result<()> {
    let mut doc = read_at(leases_file(env));
    doc[lease_id] = json!({ "provider": provider, "id": id });
    write_at(env, leases_file(env), &doc)
}

pub fn lease_find(env: &Env, lease_id: &str) -> Option<(String, String)> {
    let doc = read_at(leases_file(env));
    let entry = doc.get(lease_id)?;
    Some((
        entry["provider"].as_str()?.to_string(),
        entry["id"].as_str()?.to_string(),
    ))
}

pub fn lease_forget(env: &Env, lease_id: &str) -> std::io::Result<()> {
    let mut doc = read_at(leases_file(env));
    if let Some(map) = doc.as_object_mut() {
        map.remove(lease_id);
    }
    write_at(env, leases_file(env), &doc)
}

pub fn touch_synced(env: &Env, now: u64) -> std::io::Result<()> {
    std::fs::create_dir_all(env.state_dir())?;
    std::fs::write(env.state_dir().join("synced"), format!("{now}\n"))
}
