use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExchangeError {
    MissingAccessToken,
    InvalidJson(String),
    IdpError {
        error: String,
        description: Option<String>,
    },
}

impl std::fmt::Display for ExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExchangeError::MissingAccessToken => write!(f, "token response missing access_token"),
            ExchangeError::InvalidJson(msg) => write!(f, "invalid token response: {msg}"),
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

pub fn encode_form_body(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            let ek: String = url::form_urlencoded::byte_serialize(k.as_bytes()).collect();
            let ev: String = url::form_urlencoded::byte_serialize(v.as_bytes()).collect();
            format!("{ek}={ev}")
        })
        .collect::<Vec<_>>()
        .join("&")
}

pub fn parse_token_response(body: &str) -> Result<TokenResponse, ExchangeError> {
    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| ExchangeError::InvalidJson(e.to_string()))?;

    if let Some(error) = map.get("error").and_then(|v| v.as_str()) {
        return Err(ExchangeError::IdpError {
            error: error.to_string(),
            description: map
                .get("error_description")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        });
    }

    let access_token = map
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or(ExchangeError::MissingAccessToken)?
        .to_string();

    let refresh_token = map
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let expires_in = map.get("expires_in").and_then(|v| v.as_u64());

    let scope = map
        .get("scope")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let token_type = map
        .get("token_type")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_in,
        scope,
        token_type,
    })
}
