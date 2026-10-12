use crate::exit::Exit;
use crate::oauth::{OAuthError, Token};
use jerk_oauth::exchange;
use jerk_oauth::expiry;

#[derive(Clone, Debug, PartialEq)]
pub struct DeviceAuthResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub fn parse_device_auth_response(body: &str) -> Result<DeviceAuthResponse, OAuthError> {
    let map: serde_json::Value = serde_json::from_str(body).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: format!("invalid device auth response: {e}"),
    })?;

    if let Some(error) = map.get("error").and_then(|v| v.as_str()) {
        let desc = map
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Err(OAuthError {
            exit: Exit::Error,
            message: format!("device auth error: {error}: {desc}"),
        });
    }

    Ok(DeviceAuthResponse {
        device_code: map
            .get("device_code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OAuthError {
                exit: Exit::Error,
                message: "missing device_code".into(),
            })?
            .into(),
        user_code: map
            .get("user_code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| OAuthError {
                exit: Exit::Error,
                message: "missing user_code".into(),
            })?
            .into(),
        verification_uri: map
            .get("verification_uri")
            .or_else(|| map.get("verification_url"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| OAuthError {
                exit: Exit::Error,
                message: "missing verification_uri".into(),
            })?
            .into(),
        expires_in: map
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(900),
        interval: map.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
    })
}

pub struct PollParams<'a> {
    pub token_url: &'a str,
    pub device_code: &'a str,
    pub client_id: &'a str,
    pub interval: u64,
    pub expires_in: u64,
    pub now_secs: u64,
    pub http_post: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub sleep: &'a dyn Fn(u64),
}

pub fn poll_token(params: &PollParams<'_>) -> Result<Token, OAuthError> {
    let mut interval = params.interval;
    let deadline_secs = params.now_secs + params.expires_in;
    let mut current_secs = params.now_secs;

    loop {
        (params.sleep)(interval);
        current_secs += interval;

        if current_secs >= deadline_secs {
            return Err(OAuthError {
                exit: Exit::Error,
                message: "device code expired".into(),
            });
        }

        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code&device_code={}&client_id={}",
            url::form_urlencoded::byte_serialize(params.device_code.as_bytes()).collect::<String>(),
            url::form_urlencoded::byte_serialize(params.client_id.as_bytes()).collect::<String>(),
        );

        let response = (params.http_post)(params.token_url, &body).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e,
        })?;

        let map: serde_json::Value = serde_json::from_str(&response).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: format!("invalid token response: {e}"),
        })?;

        if let Some(error) = map.get("error").and_then(|v| v.as_str()) {
            match error {
                "authorization_pending" => continue,
                "slow_down" => {
                    interval = interval.saturating_add(5);
                    continue;
                }
                "expired_token" => {
                    return Err(OAuthError {
                        exit: Exit::Error,
                        message: "device code expired".into(),
                    });
                }
                _ => {
                    let desc = map
                        .get("error_description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    return Err(OAuthError {
                        exit: Exit::Error,
                        message: format!("token error: {error}: {desc}"),
                    });
                }
            }
        }

        let token_response = exchange::parse_token_response(&response).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e.to_string(),
        })?;
        let expires_at = expiry::compute_expiry(current_secs, token_response.expires_in, None);
        return Ok(Token {
            access_token: token_response.access_token,
            refresh_token: token_response.refresh_token,
            expires_at,
            scope: token_response.scope,
            token_type: token_response.token_type,
        });
    }
}
