use crate::paths::Env;
use serde_json::{json, Value};
use std::path::PathBuf;

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
fn restrict(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_: &std::path::Path) -> std::io::Result<()> {
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
