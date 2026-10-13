use crate::capture::sanitize_error_description;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use zeroize::Zeroize;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub token_type: String,
}

impl Drop for TokenResponse {
    fn drop(&mut self) {
        self.access_token.zeroize();
        if let Some(ref mut rt) = self.refresh_token {
            rt.zeroize();
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeError {
    MissingAccessToken,
    MissingTokenType,
    EmptyAccessToken,
    InvalidJson(String),
    InvalidTokenType,
    InvalidExpiresIn(String),
    ErrorWithToken,
    IdpError {
        error: String,
        description: Option<String>,
    },
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeError::MissingAccessToken => write!(f, "token response missing access_token"),
            ExchangeError::MissingTokenType => write!(f, "token_type must be Bearer"),
            ExchangeError::EmptyAccessToken => write!(f, "token response has empty access_token"),
            ExchangeError::InvalidJson(_) => write!(f, "invalid token response JSON"),
            ExchangeError::InvalidTokenType => {
                write!(f, "token_type must be Bearer")
            }
            ExchangeError::InvalidExpiresIn(val) => {
                write!(f, "invalid expires_in: {val}")
            }
            ExchangeError::ErrorWithToken => {
                write!(f, "token response contains both error and access_token")
            }
            ExchangeError::IdpError { error, description } => match description {
                Some(desc) => write!(f, "token endpoint error: {error}: {desc}"),
                None => write!(f, "token endpoint error: {error}"),
            },
        }
    }
}

pub fn build_authorization_code_body(
    code: &str,
    client_id: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Vec<(String, String)> {
    vec![
        ("grant_type".into(), "authorization_code".into()),
        ("code".into(), code.into()),
        ("client_id".into(), client_id.into()),
        ("redirect_uri".into(), redirect_uri.into()),
        ("code_verifier".into(), verifier.into()),
    ]
}

pub fn build_refresh_body(refresh_token: &str, client_id: &str) -> Vec<(String, String)> {
    vec![
        ("grant_type".into(), "refresh_token".into()),
        ("refresh_token".into(), refresh_token.into()),
        ("client_id".into(), client_id.into()),
    ]
}

pub struct RefreshParams<'a> {
    pub token_url: &'a str,
    pub client_id: &'a str,
    pub refresh_token: &'a str,
    pub scope: Option<&'a str>,
    pub resource: Option<&'a str>,
    pub client_secret: Option<&'a str>,
}

pub fn refresh_request(params: &RefreshParams<'_>) -> Result<(String, String), String> {
    crate::config::require_https_or_loopback(params.token_url)
        .map_err(|e| format!("token_url {e}"))?;
    if params.client_id.is_empty() {
        return Err("client_id must not be empty".into());
    }
    if params.refresh_token.is_empty() {
        return Err("refresh_token must not be empty".into());
    }
    if let Some(resource) = params.resource {
        crate::config::require_https_or_loopback(resource).map_err(|e| format!("resource {e}"))?;
        if resource.contains('#') {
            return Err("resource must not contain a fragment".into());
        }
    }
    let mut pairs = build_refresh_body(params.refresh_token, params.client_id);
    if let Some(scope) = params.scope.filter(|s| !s.trim().is_empty()) {
        pairs.push(("scope".into(), scope.into()));
    }
    if let Some(resource) = params.resource {
        pairs.push(("resource".into(), resource.into()));
    }
    if let Some(secret) = params.client_secret.filter(|s| !s.is_empty()) {
        pairs.push(("client_secret".into(), secret.into()));
    }
    let body = encode_form_body(&pairs);
    for (_, v) in pairs.iter_mut() {
        v.zeroize();
    }
    let url = url::Url::parse(params.token_url)
        .map(|u| u.to_string())
        .map_err(|_| "token_url must be an absolute URL".to_string())?;
    Ok((url, body))
}

pub fn encode_form_body(pairs: &[(String, String)]) -> String {
    let capacity = pairs.iter().map(|(k, v)| 3 * (k.len() + v.len()) + 2).sum();
    let mut out = String::with_capacity(capacity);
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        out.extend(url::form_urlencoded::byte_serialize(k.as_bytes()));
        out.push('=');
        out.extend(url::form_urlencoded::byte_serialize(v.as_bytes()));
    }
    out
}

pub fn parse_token_response(body: &str) -> Result<TokenResponse, ExchangeError> {
    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|_| ExchangeError::InvalidJson("parse error".into()))?;

    let has_error = map.get("error").and_then(|v| v.as_str()).is_some();
    let has_token = map.get("access_token").and_then(|v| v.as_str()).is_some();

    if has_error && has_token {
        return Err(ExchangeError::ErrorWithToken);
    }

    if has_error {
        let error = map
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let description = map
            .get("error_description")
            .and_then(|v| v.as_str())
            .map(sanitize_error_description);
        return Err(ExchangeError::IdpError {
            error: sanitize_error_description(&error),
            description,
        });
    }

    if let Some(error_val) = map.get("error") {
        if !error_val.is_null() {
            return Err(ExchangeError::ErrorWithToken);
        }
    }

    let access_token = map
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(ExchangeError::MissingAccessToken)?
        .to_string();

    if access_token.is_empty() {
        return Err(ExchangeError::EmptyAccessToken);
    }

    if let Some(tt_val) = map.get("token_type") {
        if let Some(tt) = tt_val.as_str() {
            if !tt.eq_ignore_ascii_case("bearer") {
                return Err(ExchangeError::InvalidTokenType);
            }
        } else {
            return Err(ExchangeError::InvalidTokenType);
        }
    } else {
        return Err(ExchangeError::MissingTokenType);
    }

    let expires_in = match map.get("expires_in") {
        Some(val) => {
            if let Some(n) = val.as_u64() {
                if n > 315_360_000 {
                    return Err(ExchangeError::InvalidExpiresIn("too large".into()));
                }
                Some(n)
            } else if val.as_i64().is_some() {
                return Err(ExchangeError::InvalidExpiresIn("negative".into()));
            } else if val.as_f64().is_some() || val.as_str().is_some() {
                return Err(ExchangeError::InvalidExpiresIn("must be an integer".into()));
            } else {
                return Err(ExchangeError::InvalidExpiresIn("unexpected type".into()));
            }
        }
        None => None,
    };

    let refresh_token = map
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let scope = map
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let token_type = map
        .get("token_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Bearer")
        .to_string();

    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_in,
        scope,
        token_type,
    })
}
