use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Loopback,
    Device,
    Intercept,
    Popup,
    Auto,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    Pkce,
    Implicit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureAt {
    Request,
    Response,
    Callback,
    Navigation,
    Network,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureFrom {
    Query,
    Fragment,
    Header,
    Url,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capture {
    pub at: CaptureAt,
    pub from: CaptureFrom,
    pub param: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rewrite {
    #[serde(rename = "match")]
    pub match_pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub append: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replace_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnCapture {
    #[default]
    Fulfill,
    Close,
    Leave,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Renew {
    pub prompt: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "source")]
pub enum Relay {
    Origin { origin: String },
    Local { port: u16 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthConfig {
    pub mode: Mode,
    pub flow: Flow,
    pub authorize_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    pub client_id: String,
    pub redirect_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub capture: Capture,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<Relay>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewrite: Vec<Rewrite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_capture: Option<OnCapture>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_param: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renew: Option<Renew>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolate: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct ValidConfig(OAuthConfig);

impl ValidConfig {
    pub fn inner(&self) -> &OAuthConfig {
        &self.0
    }
}

impl std::ops::Deref for ValidConfig {
    type Target = OAuthConfig;
    fn deref(&self) -> &OAuthConfig {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

pub fn require_https_or_loopback(url_str: &str) -> Result<(), String> {
    match url::Url::parse(url_str) {
        Ok(parsed) => {
            if parsed.username() != "" || parsed.password().is_some() {
                return Err("must not contain userinfo".into());
            }
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("");
            if scheme == "https" {
                return Ok(());
            }
            if scheme == "http" && is_loopback_host(host) {
                return Ok(());
            }
            Err(format!(
                "must use https (http only for loopback), got {scheme}://{host}"
            ))
        }
        Err(_) => Err("must be an absolute URL".into()),
    }
}

fn validate_url_scheme(url_str: &str, field: &str, errors: &mut Vec<ValidationError>) {
    match url::Url::parse(url_str) {
        Ok(parsed) => {
            if parsed.username() != "" || parsed.password().is_some() {
                errors.push(ValidationError {
                    field: field.into(),
                    message: "must not contain userinfo".into(),
                });
                return;
            }
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("");
            if scheme == "https" {
                return;
            }
            if scheme == "http" && is_loopback_host(host) {
                return;
            }
            errors.push(ValidationError {
                field: field.into(),
                message: format!(
                    "must use https (http is only allowed for loopback hosts), got {scheme}://{host}"
                ),
            });
        }
        Err(_) => {
            errors.push(ValidationError {
                field: field.into(),
                message: "must be an absolute URL".into(),
            });
        }
    }
}

pub fn validate(config: &OAuthConfig) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    if config.authorize_url.is_empty() {
        errors.push(ValidationError {
            field: "authorize_url".into(),
            message: "must not be empty".into(),
        });
    } else {
        let raw = strip_placeholders(&config.authorize_url);
        validate_url_scheme(&raw, "authorize_url", &mut errors);
    }

    if config.client_id.is_empty() {
        errors.push(ValidationError {
            field: "client_id".into(),
            message: "must not be empty".into(),
        });
    }

    if config.redirect_uri.is_empty() {
        errors.push(ValidationError {
            field: "redirect_uri".into(),
            message: "must not be empty".into(),
        });
    } else {
        validate_url_scheme(&config.redirect_uri, "redirect_uri", &mut errors);
    }

    if let Some(ref token_url) = config.token_url {
        if token_url.is_empty() {
            errors.push(ValidationError {
                field: "token_url".into(),
                message: "must not be empty".into(),
            });
        } else {
            validate_url_scheme(token_url, "token_url", &mut errors);
        }
    }

    if config.capture.param.is_empty() {
        errors.push(ValidationError {
            field: "capture.param".into(),
            message: "must not be empty".into(),
        });
    }

    if config.flow == Flow::Pkce && config.token_url.is_none() {
        errors.push(ValidationError {
            field: "token_url".into(),
            message: "required for pkce flow".into(),
        });
    }

    if config.capture.at == CaptureAt::Network && config.capture.url.is_none() {
        errors.push(ValidationError {
            field: "capture.url".into(),
            message: "required when capture.at is network".into(),
        });
    }

    if config.capture.at == CaptureAt::Network
        && config.capture.from == CaptureFrom::Header
        && config.capture.header.is_none()
    {
        errors.push(ValidationError {
            field: "capture.header".into(),
            message: "required when capture.from is header".into(),
        });
    }

    if let Some(relay) = &config.relay {
        match relay {
            Relay::Origin { origin } => {
                if origin.is_empty() {
                    errors.push(ValidationError {
                        field: "relay.origin".into(),
                        message: "must not be empty".into(),
                    });
                } else if url::Url::parse(origin).is_err() {
                    errors.push(ValidationError {
                        field: "relay.origin".into(),
                        message: "must be a valid URL".into(),
                    });
                }
            }
            Relay::Local { port } => {
                if *port < 1024 {
                    errors.push(ValidationError {
                        field: "relay.port".into(),
                        message: "must be 1024 or higher".into(),
                    });
                }
            }
        }
    }

    if let Some(timeout) = &config.timeout {
        if parse_timeout(timeout).is_none() {
            errors.push(ValidationError {
                field: "timeout".into(),
                message: format!("{timeout:?} is not a valid duration such as 2m"),
            });
        }
    }

    for (i, rule) in config.rewrite.iter().enumerate() {
        if rule.match_pattern.is_empty() {
            errors.push(ValidationError {
                field: format!("rewrite[{i}].match"),
                message: "must not be empty".into(),
            });
        }
        if rule.append.is_none() && rule.replace_url.is_none() {
            errors.push(ValidationError {
                field: format!("rewrite[{i}]"),
                message: "needs append or replace_url".into(),
            });
        }
    }

    errors
}

pub fn validated(config: OAuthConfig) -> Result<ValidConfig, Vec<ValidationError>> {
    let errors = validate(&config);
    if errors.is_empty() {
        Ok(ValidConfig(config))
    } else {
        Err(errors)
    }
}

fn strip_placeholders(url: &str) -> String {
    let mut result = String::with_capacity(url.len());
    let mut in_placeholder = false;
    for ch in url.chars() {
        if ch == '{' {
            in_placeholder = true;
            result.push_str("_placeholder_");
        } else if ch == '}' {
            in_placeholder = false;
        } else if !in_placeholder {
            result.push(ch);
        }
    }
    result
}

pub fn parse_timeout(text: &str) -> Option<u64> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let n: u64 = number.parse().ok()?;
    match unit {
        "ms" => Some(n),
        "s" => n.checked_mul(1000),
        "m" => n.checked_mul(60_000),
        "h" => n.checked_mul(3_600_000),
        _ => None,
    }
}

pub fn default_timeout_ms() -> u64 {
    120_000
}

pub fn timeout_ms(config: &OAuthConfig) -> u64 {
    config
        .timeout
        .as_deref()
        .and_then(parse_timeout)
        .unwrap_or_else(default_timeout_ms)
}

pub fn validate_entropy(random32: &[u8; 32], random16: &[u8; 16]) -> Result<(), &'static str> {
    if random32.iter().all(|&b| b == random32[0]) {
        return Err("random32 failed a sanity check; the caller must supply CSPRNG bytes");
    }
    if random16.iter().all(|&b| b == random16[0]) {
        return Err("random16 failed a sanity check; the caller must supply CSPRNG bytes");
    }
    Ok(())
}
