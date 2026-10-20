use crate::mask::mask;
use crate::time;
use base64::Engine;
use serde_json::Value;
use std::collections::BTreeMap;

pub fn token(stdout: &str, parse: Option<&str>) -> Option<String> {
    let value = match parse.unwrap_or("text") {
        "text" | "jwt" => stdout.trim().to_string(),
        other if other.starts_with("scan:") => scan(stdout, &other[5..])?,
        other => match json(stdout, other)? {
            Value::String(s) => s.trim().to_string(),
            _ => return None,
        },
    };
    (!value.is_empty() && !value.contains(char::is_whitespace)).then_some(value)
}

fn token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "._-~+/=".contains(c)
}

fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => while chars.next().is_some_and(|c| !('@'..='~').contains(&c)) {},
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => out.push('\n'),
            c => out.push(c),
        }
    }
    out
}

fn scan(stdout: &str, prefix: &str) -> Option<String> {
    let text = plain(stdout);
    let at = text.rfind(prefix)?;
    let mut lines = text[at..].lines();
    let first = lines.next().unwrap_or_default();
    let mut value: String = first.chars().take_while(|c| token_char(*c)).collect();
    if first[value.len()..].trim().is_empty() {
        for line in lines {
            let piece = line.trim();
            if piece.is_empty() || !piece.chars().all(token_char) {
                break;
            }
            value.push_str(piece);
        }
    }
    Some(value)
}

pub fn json(stdout: &str, parse: &str) -> Option<Value> {
    let pointer = parse.strip_prefix("json:")?;
    let doc: Value = serde_json::from_str(stdout.trim()).ok()?;
    doc.pointer(pointer).cloned()
}

pub fn fields(stdout: &str, map: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (name, parse) in map {
        if let Some(value) = token(stdout, Some(parse)) {
            out.insert(name.clone(), value);
        }
    }
    out
}

pub fn expiry(stdout: &str, token_value: &str, expiry: Option<&str>) -> Option<u64> {
    match expiry {
        Some("jwt") => jwt_expiry(token_value),
        Some(parse) if parse.starts_with("json:") => match json(stdout, parse)? {
            Value::String(s) => time::parse_rfc3339(&s),
            Value::Number(n) => n.as_u64(),
            _ => None,
        },
        _ => None,
    }
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

fn jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn jwt_scopes(token: &str) -> Option<String> {
    let claims = jwt_claims(token)?;
    ["scp", "scope"]
        .iter()
        .find_map(|key| claims.get(*key).and_then(scopes))
}

pub fn jwt_expiry(token: &str) -> Option<u64> {
    let claims = jwt_claims(token)?;
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
