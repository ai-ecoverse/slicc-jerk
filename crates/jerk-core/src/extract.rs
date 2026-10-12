use crate::mask::mask;
use base64::Engine;
use serde_json::Value;

pub fn token(stdout: &str, parse: Option<&str>) -> Option<String> {
    let value = match parse.unwrap_or("text") {
        "text" | "jwt" => stdout.trim().to_string(),
        other => match json(stdout, other)? {
            Value::String(s) => s.trim().to_string(),
            _ => return None,
        },
    };
    (!value.is_empty() && !value.contains(char::is_whitespace)).then_some(value)
}

pub fn json(stdout: &str, parse: &str) -> Option<Value> {
    let pointer = parse.strip_prefix("json:")?;
    let doc: Value = serde_json::from_str(stdout.trim()).ok()?;
    doc.pointer(pointer).cloned()
}

pub fn scopes(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Array(items) => {
            let parts: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            (!parts.is_empty()).then(|| parts.join(","))
        }
        _ => None,
    }
}

pub fn jwt_expiry(token: &str) -> Option<u64> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    if let Some(exp) = claims.get("exp").and_then(Value::as_u64) {
        return Some(exp);
    }
    let number = |key: &str| match claims.get(key)? {
        Value::String(s) => s.parse::<u64>().ok(),
        other => other.as_u64(),
    };
    Some((number("created_at")? + number("expires_in")?) / 1000)
}

const SECRET_KEYS: [&str; 4] = ["token", "secret", "password", "apikey"];

pub fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| {
                    let lower = k.to_ascii_lowercase().replace(['_', '-'], "");
                    let secret = SECRET_KEYS.iter().any(|s| lower.contains(s));
                    let v = match v {
                        Value::String(s) if secret => Value::String(mask(s)),
                        other => redact(other),
                    };
                    (k.clone(), v)
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        other => other.clone(),
    }
}
