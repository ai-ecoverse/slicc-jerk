use crate::sealed::Sealed;
use serde::Deserialize;
use serde_json::{json, Value};
use zeroize::Zeroize;

const MAX_TIME: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: &'static str,
    pub cause: Option<&'static str>,
}

impl Error {
    pub const fn new(code: &'static str) -> Self {
        Self { code, cause: None }
    }

    pub fn exit(self) -> crate::exit::Exit {
        use crate::exit::Exit;
        match (self.code, self.cause) {
            (_, Some("oauth-reauth-required")) | ("ENOENT", _) => Exit::SignIn,
            ("ENOTSUP", _) => Exit::Unsupported,
            ("ENOTCONN" | "ETIMEDOUT" | "EAGAIN", _) => Exit::Unavailable,
            _ => Exit::Error,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match (self.code, self.cause) {
            (_, Some("vault-locked")) => "Vault is locked",
            (_, Some("oauth-reauth-required")) => "OAuth sign-in is required; run create again",
            (_, Some("oauth-refresh-failed")) => "OAuth refresh failed",
            ("EINVAL", _) => "Invalid OAuth request",
            ("ENOENT", _) => "No managed OAuth record",
            ("EEXIST", _) => "OAuth record ownership or binding does not match",
            ("EACCES", _) => "OAuth operation denied",
            ("EAGAIN", _) => "OAuth record changed; retry the command",
            ("ENOTSUP", _) => "Managed OAuth refresh is unsupported; update kernel, bios and vault",
            ("ENOTCONN", _) => "Vault is unavailable",
            ("ETIMEDOUT", _) => "OAuth operation timed out",
            _ => "Invalid OAuth response",
        };
        write!(f, "{message}")
    }
}

fn exact(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.len() == keys.len() && keys.iter().all(|key| map.contains_key(*key)))
}

fn error(reply: &Value) -> Result<(), Error> {
    let bad = Error::new("EIO");
    if reply["v"].as_u64() != Some(1) {
        return Err(bad);
    }
    let Some(value) = reply.get("error") else {
        return Ok(());
    };
    if !exact(reply, &["v", "error"])
        || !(exact(value, &["code", "message"]) || exact(value, &["code", "message", "cause"]))
        || !value["message"].is_string()
    {
        return Err(bad);
    }
    let code = match value["code"].as_str() {
        Some("EINVAL") => "EINVAL",
        Some("ENOENT") => "ENOENT",
        Some("EEXIST") => "EEXIST",
        Some("EACCES") => "EACCES",
        Some("EAGAIN") => "EAGAIN",
        Some("ENOTSUP") => "ENOTSUP",
        Some("ENOTCONN") => "ENOTCONN",
        Some("ETIMEDOUT") => "ETIMEDOUT",
        Some("EIO") => "EIO",
        _ => return Err(bad),
    };
    let cause = match value.get("cause") {
        None => None,
        Some(Value::String(s)) if s == "vault-locked" => Some("vault-locked"),
        Some(Value::String(s)) if s == "oauth-reauth-required" => Some("oauth-reauth-required"),
        Some(Value::String(s)) if s == "oauth-refresh-failed" => Some("oauth-refresh-failed"),
        _ => return Err(bad),
    };
    Err(Error { code, cause })
}

pub fn request(sealed: &dyn Sealed, body: &Value) -> Result<Value, Error> {
    let mut reply = sealed
        .request(body)
        .map_err(|error| Error::new(error.code))?;
    if let Err(e) = error(&reply) {
        wipe(&mut reply);
        return Err(e);
    }
    Ok(reply)
}

pub fn capabilities(sealed: &dyn Sealed) -> Result<(), Error> {
    let mut reply = match request(sealed, &json!({"v": 1, "op": "capabilities"})) {
        Err(e) if e.code == "EINVAL" => return Err(Error::new("ENOTSUP")),
        other => other?,
    };
    let result = if exact(&reply, &["v", "capabilities"]) && exact(&reply["capabilities"], &[]) {
        Err(Error::new("ENOTSUP"))
    } else if !exact(&reply, &["v", "capabilities"])
        || !exact(&reply["capabilities"], &["kernelOAuth"])
        || reply["capabilities"]["kernelOAuth"].as_u64() != Some(1)
    {
        Err(Error::new("EIO"))
    } else {
        Ok(())
    };
    wipe(&mut reply);
    result
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Ready,
    Refreshing,
    Expired,
    ReauthRequired,
    Removed,
    Locked,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Status {
    pub v: u64,
    pub placeholder: String,
    pub state: State,
    pub expires_at: Option<u64>,
    pub refreshable: bool,
}

pub fn placeholder(value: &str) -> bool {
    value.strip_prefix("fnox_proxy_").is_some_and(|s| {
        s.len() == 24
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

pub fn status(sealed: &dyn Sealed, body: &Value) -> Result<Status, Error> {
    let mut reply = request(sealed, body)?;
    let parsed = if exact(
        &reply,
        &["v", "placeholder", "state", "expiresAt", "refreshable"],
    ) {
        serde_json::from_value::<Status>(reply.clone()).ok()
    } else {
        None
    };
    wipe(&mut reply);
    parsed
        .filter(|s| placeholder(&s.placeholder) && s.expires_at.is_none_or(|t| t <= MAX_TIME))
        .ok_or(Error::new("EIO"))
}

pub fn remove(sealed: &dyn Sealed, body: &Value) -> Result<(), Error> {
    let mut reply = request(sealed, body)?;
    let result = if exact(&reply, &["v", "removed"]) && reply["removed"] == true {
        Ok(())
    } else {
        Err(Error::new("EIO"))
    };
    wipe(&mut reply);
    result
}

pub fn millis(seconds: u64) -> Result<u64, Error> {
    seconds
        .checked_mul(1000)
        .filter(|t| *t <= MAX_TIME)
        .ok_or(Error::new("EINVAL"))
}

pub fn wipe(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(fields) => fields.values_mut().for_each(wipe),
        _ => {}
    }
}
