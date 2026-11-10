use super::oauth_protocol::{self as protocol, Error, State, Status};
use crate::config::Entry;
use crate::paths::Env;
use crate::sealed::Sealed;
use jerk_oauth::config::{Flow, Mode, OAuthConfig};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zeroize::Zeroize;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    provider: String,
    account: String,
    name: String,
    placeholder: Option<String>,
}

fn path(env: &Env, provider: &str, account: &str) -> std::path::PathBuf {
    crate::grants::file_store::FileStore::new(env.state_dir().join("oauth-managed"))
        .path(provider, account)
}

fn reference(env: &Env, provider: &str, account: &str) -> Result<Option<Reference>, Error> {
    let bytes = match std::fs::read(path(env, provider, account)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::new("EIO")),
    };
    let saved: Reference = serde_json::from_slice(&bytes).map_err(|_| Error::new("EIO"))?;
    if saved.provider != provider
        || saved.account != account
        || saved
            .placeholder
            .as_deref()
            .is_some_and(|value| !protocol::placeholder(value))
    {
        return Err(Error::new("EIO"));
    }
    Ok(Some(saved))
}

fn identity(op: &str, name: &str, provider: &str, account: &str) -> Value {
    json!({"v": 1, "op": op, "name": name, "provider": provider, "account": account})
}

fn marked(value: &Value) -> Result<bool, Error> {
    match value.get("oauthManaged") {
        None => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        _ => Err(Error::new("EIO")),
    }
}

pub fn locate(
    entry: &Entry,
    account: &str,
    env: &Env,
    sealed: &dyn Sealed,
    use_env: bool,
) -> Result<Option<String>, Error> {
    let selected = use_env
        .then(|| {
            entry
                .provider
                .env
                .iter()
                .find_map(|name| env.var(name).map(|value| (name, value)))
        })
        .flatten();
    if selected.is_some_and(|(_, value)| !protocol::placeholder(value)) {
        return Ok(None);
    }
    let saved = reference(env, &entry.id, account)?;
    if !use_env {
        if let Some(saved) = saved {
            return Ok(Some(saved.name));
        }
    }
    if let Some((name, value)) = selected {
        if saved
            .as_ref()
            .is_some_and(|r| r.placeholder.as_deref() == Some(value))
        {
            return Ok(saved.map(|r| r.name));
        }
        let record = sealed
            .request(&json!({"v": 1, "op": "placeholder", "name": name}))
            .map_err(|_| Error::new("ENOTCONN"))?;
        return Ok(
            (marked(&record)? && record["placeholder"].as_str() == Some(value))
                .then(|| name.clone()),
        );
    }
    for name in &entry.provider.env {
        if saved.as_ref().is_some_and(|r| r.name == *name) {
            return Ok(Some(name.clone()));
        }
        let Ok(record) = sealed.request(&json!({"v": 1, "op": "placeholder", "name": name})) else {
            continue;
        };
        if marked(&record)? {
            return Ok(Some(name.clone()));
        }
        if record["state"] != "removed"
            && record["placeholder"]
                .as_str()
                .is_some_and(crate::sealed::is_placeholder)
        {
            return Ok(None);
        }
    }
    Ok(saved.map(|r| r.name))
}

pub fn read_status(
    entry: &Entry,
    account: &str,
    name: &str,
    sealed: &dyn Sealed,
) -> Result<Status, Error> {
    protocol::capabilities(sealed)?;
    protocol::status(sealed, &identity("oauthStatus", name, &entry.id, account))
}

pub fn ready(
    entry: &Entry,
    account: &str,
    name: &str,
    sealed: &dyn Sealed,
    now: u64,
    force: bool,
) -> Result<Status, Error> {
    let now = protocol::millis(now)?;
    let mut status = read_status(entry, account, name, sealed)?;
    if force && !status.refreshable && matches!(status.state, State::Ready | State::Expired) {
        return Err(Error::new("ENOTSUP"));
    }
    usable(&status)?;
    if force
        || status.state == State::Refreshing
        || status
            .expires_at
            .is_some_and(|t| t <= now.saturating_add(300_000))
    {
        if force && !status.refreshable {
            return Err(Error::new("ENOTSUP"));
        }
        if status.refreshable {
            let refreshed =
                protocol::status(sealed, &identity("oauthRefresh", name, &entry.id, account))?;
            if refreshed.placeholder != status.placeholder {
                return Err(Error::new("EIO"));
            }
            status = refreshed;
            usable(&status)?;
        }
    }
    if status.state != State::Ready || status.expires_at.is_some_and(|t| t <= now) {
        return Err(Error {
            code: "EACCES",
            cause: Some("oauth-reauth-required"),
        });
    }
    Ok(status)
}

fn usable(status: &Status) -> Result<(), Error> {
    match status.state {
        State::Ready | State::Refreshing => Ok(()),
        State::Locked => Err(Error {
            code: "EACCES",
            cause: Some("vault-locked"),
        }),
        _ => Err(Error {
            code: "EACCES",
            cause: Some("oauth-reauth-required"),
        }),
    }
}

pub fn lookup(
    entry: &Entry,
    account: &str,
    env: &Env,
    sealed: &dyn Sealed,
    now: u64,
    force: bool,
) -> Result<Option<Status>, Error> {
    match locate(entry, account, env, sealed, true)? {
        Some(name) => ready(entry, account, &name, sealed, now, force).map(Some),
        None if force => Err(Error::new("ENOTSUP")),
        None => Ok(None),
    }
}

pub fn eligible(config: &OAuthConfig) -> bool {
    config.mode != Mode::ClientCredentials
        && matches!(config.flow, Flow::Pkce | Flow::Code)
        && config.client_secret.as_deref().is_none_or(str::is_empty)
}

pub fn prepare(config: &OAuthConfig, sealed: &dyn Sealed) -> Result<bool, Error> {
    if !eligible(config) {
        return Ok(false);
    }
    if !config.capture.fields.is_empty() {
        return Err(Error::new("ENOTSUP"));
    }
    protocol::capabilities(sealed)?;
    Ok(true)
}

pub fn refresh_env(config: &OAuthConfig, env: &Env) -> bool {
    config
        .refresh_env
        .as_deref()
        .and_then(|name| env.var(name))
        .is_some()
}

pub fn adopt(
    entry: &Entry,
    account: &str,
    config: &OAuthConfig,
    scope: Option<&str>,
    mut token: crate::oauth::Token,
    env: &Env,
    sealed: &dyn Sealed,
) -> Result<(), Error> {
    let result = (|| {
        let name = entry.provider.env.first().ok_or(Error::new("EINVAL"))?;
        if !eligible(config) || !config.capture.fields.is_empty() || !token.fields.is_empty() {
            return Err(Error::new("ENOTSUP"));
        }
        if [entry.id.as_str(), account, name.as_str()]
            .iter()
            .any(|value| {
                [
                    token.access_token.as_str(),
                    token.refresh_token.as_deref().unwrap_or(""),
                ]
                .iter()
                .any(|secret| !secret.is_empty() && value.contains(*secret))
            })
        {
            return Err(Error::new("EINVAL"));
        }
        let saved = Reference {
            provider: entry.id.clone(),
            account: account.into(),
            name: name.clone(),
            placeholder: None,
        };
        save(env, &saved)?;
        let mut body = identity("oauthGrant", name, &entry.id, account);
        body["accessToken"] = json!(token.access_token);
        body["oauth"] = json!({"tokenUrl": config.token_url, "clientId": config.client_id, "authMethod": "none"});
        if let Some(scope) = token.scope.as_deref().or(scope) {
            body["oauth"]["scope"] = json!(scope);
        }
        if let Some(value) = &token.refresh_token {
            body["refreshToken"] = json!(value);
        }
        if let Some(value) = &token.token_type {
            body["tokenType"] = json!(value);
        }
        let expires = token.expires_at.map(protocol::millis).transpose();
        let result = match expires {
            Ok(expires) => {
                if let Some(expires) = expires {
                    body["expiresAt"] = json!(expires);
                }
                protocol::status(sealed, &body)
            }
            Err(error) => Err(error),
        };
        protocol::wipe(&mut body);
        let status = result?;
        usable(&status)?;
        let saved = Reference {
            placeholder: Some(status.placeholder),
            ..saved
        };
        save(env, &saved)
    })();
    token.access_token.zeroize();
    token.refresh_token.zeroize();
    token.scope.zeroize();
    token.token_type.zeroize();
    for value in token.fields.values_mut() {
        value.zeroize();
    }
    result
}

fn save(env: &Env, saved: &Reference) -> Result<(), Error> {
    let path = path(env, &saved.provider, &saved.account);
    std::fs::create_dir_all(path.parent().ok_or(Error::new("EIO"))?)
        .map_err(|_| Error::new("EIO"))?;
    std::fs::write(
        path,
        serde_json::to_vec(saved).map_err(|_| Error::new("EIO"))?,
    )
    .map_err(|_| Error::new("EIO"))
}

pub fn remove(
    entry: &Entry,
    account: &str,
    name: &str,
    env: &Env,
    sealed: &dyn Sealed,
) -> Result<(), Error> {
    protocol::capabilities(sealed)?;
    protocol::remove(sealed, &identity("oauthRemove", name, &entry.id, account))?;
    match std::fs::remove_file(path(env, &entry.id, account)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::new("EIO")),
    }
}

pub fn drop_if_managed(
    entry: &Entry,
    account: &str,
    yes: bool,
    ctx: &mut crate::cli::Ctx<'_>,
    sealed: &dyn Sealed,
    io: &mut crate::cli::Io<'_>,
) -> Option<crate::exit::Exit> {
    use crate::exit::Exit;
    let name = match locate(entry, account, ctx.env, sealed, false) {
        Ok(name) => name?,
        Err(error) => return Some(report(entry, error, io)),
    };
    if !yes {
        if !ctx.stdin_terminal {
            return Some(crate::cli::usage(
                "drop: pass --yes when not on a terminal",
                io,
            ));
        }
        let _ = write!(io.err, "Drop {}? [y/N] ", entry.id);
        let _ = io.err.flush();
        let mut answer = String::new();
        let _ = ctx.input.read_line(&mut answer);
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            let _ = writeln!(io.err, "jerk: cancelled");
            return Some(Exit::Error);
        }
    }
    if let Err(error) = remove(entry, account, &name, ctx.env, sealed) {
        return Some(report(entry, error, io));
    }
    if ctx
        .oauth
        .grants(entry.provider.store.as_deref())
        .remove(&entry.id, account)
        .is_err()
    {
        let _ = writeln!(io.err, "jerk: cannot remove local OAuth grant");
        return Some(Exit::Error);
    }
    let _ = writeln!(io.out, "{}: dropped", entry.id);
    Some(Exit::Ok)
}

pub fn show(entry: &Entry, status: &Status, json: bool, io: &mut crate::cli::Io<'_>) {
    if json {
        let _ = writeln!(
            io.out,
            "{}",
            json!({"provider": entry.id, "source": "oauth", "placeholder": status.placeholder,
            "expires_at": status.expires_at.map(|t| t / 1000), "state": "ready", "refreshable": status.refreshable})
        );
    } else {
        let _ = writeln!(io.out, "{}", status.placeholder);
    }
}

pub fn report(entry: &Entry, error: Error, io: &mut crate::cli::Io<'_>) -> crate::exit::Exit {
    let _ = writeln!(io.err, "jerk: {}: {error}", entry.id);
    error.exit()
}
