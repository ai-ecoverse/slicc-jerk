use crate::paths::Env;
use serde_json::{json, Value};
use std::path::PathBuf;

fn file(env: &Env) -> PathBuf {
    env.state_dir().join("minted.json")
}

fn read(env: &Env) -> Value {
    std::fs::read_to_string(file(env))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn write(env: &Env, doc: &Value) -> std::io::Result<()> {
    let path = file(env);
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
