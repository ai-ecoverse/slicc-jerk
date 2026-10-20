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
    ClientCredentials,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flow {
    #[default]
    Pkce,
    Code,
    Implicit,
    Exchange,
    ClientCredentials,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_type: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub body: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_form")]
    pub format: ExchangeFormat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_field: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeFormat {
    #[default]
    Form,
    Json,
}

fn is_form(format: &ExchangeFormat) -> bool {
    *format == ExchangeFormat::Form
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
    Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStage {
    Request,
    Response,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<CaptureStage>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

impl Default for Capture {
    fn default() -> Self {
        Self {
            at: CaptureAt::Request,
            from: CaptureFrom::Query,
            param: String::new(),
            url: None,
            header: None,
            stage: None,
            fields: BTreeMap::new(),
        }
    }
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
    #[serde(default)]
    pub flow: Flow,
    #[serde(default)]
    pub authorize_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub redirect_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoke_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_env: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange: Option<Exchange>,
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
            Err("must use https (http only for loopback)".into())
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
                message: "must use https (http is only allowed for loopback hosts)".into(),
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
    let machine = config.mode == Mode::ClientCredentials || config.flow == Flow::ClientCredentials;

    if config.client_id.is_empty() {
        errors.push(ValidationError {
            field: "client_id".into(),
            message: "must not be empty".into(),
        });
    }

    if machine {
        match &config.token_url {
            None => errors.push(ValidationError {
                field: "token_url".into(),
                message: "required for client_credentials".into(),
            }),
            Some(token_url) if token_url.is_empty() => errors.push(ValidationError {
                field: "token_url".into(),
                message: "must not be empty".into(),
            }),
            Some(token_url) => {
                let raw = strip_placeholders(token_url);
                validate_url_scheme(&raw, "token_url", &mut errors);
            }
        }
        match &config.client_secret {
            None => errors.push(ValidationError {
                field: "client_secret".into(),
                message: "required for client_credentials".into(),
            }),
            Some(secret) if secret.is_empty() => errors.push(ValidationError {
                field: "client_secret".into(),
                message: "must not be empty".into(),
            }),
            Some(_) => {}
        }
    } else {
        if config.authorize_url.is_empty() {
            errors.push(ValidationError {
                field: "authorize_url".into(),
                message: "must not be empty".into(),
            });
        } else {
            let raw = strip_placeholders(&config.authorize_url);
            validate_url_scheme(&raw, "authorize_url", &mut errors);
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

        if matches!(config.flow, Flow::Pkce | Flow::Code) && config.token_url.is_none() {
            errors.push(ValidationError {
                field: "token_url".into(),
                message: match config.flow {
                    Flow::Pkce => "required for pkce flow".into(),
                    Flow::Code => "required for code flow".into(),
                    Flow::Exchange | Flow::Implicit | Flow::ClientCredentials => unreachable!(),
                },
            });
        }

        if config.flow == Flow::Exchange {
            if config.token_url.is_none() {
                errors.push(ValidationError {
                    field: "token_url".into(),
                    message: "required for exchange flow".into(),
                });
            }
            match &config.exchange {
                None => errors.push(ValidationError {
                    field: "exchange".into(),
                    message: "required for exchange flow".into(),
                }),
                Some(ex) if ex.grant_type.as_deref().unwrap_or("").is_empty() => {
                    errors.push(ValidationError {
                        field: "exchange.grant_type".into(),
                        message: "must not be empty".into(),
                    })
                }
                Some(_) => {}
            }
        }

        validate_exchange(config, &mut errors);

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
    }

    if let Some(ref revoke_url) = config.revoke_url {
        if revoke_url.is_empty() {
            errors.push(ValidationError {
                field: "revoke_url".into(),
                message: "must not be empty".into(),
            });
        } else {
            validate_url_scheme(revoke_url, "revoke_url", &mut errors);
        }
    }

    if let Some(ref refresh_env) = config.refresh_env {
        if refresh_env.is_empty() {
            errors.push(ValidationError {
                field: "refresh_env".into(),
                message: "must not be empty".into(),
            });
        }
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
                message: "is not a valid duration such as 2m".into(),
            });
        }
    }

    validate_capture_extra(config, &mut errors);
    reject_unavailable_capture_builtins(config, &mut errors);

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

fn validate_exchange(config: &OAuthConfig, errors: &mut Vec<ValidationError>) {
    let Some(ex) = &config.exchange else {
        return;
    };
    if config.flow == Flow::Implicit {
        errors.push(ValidationError {
            field: "exchange".into(),
            message: "only applies to the exchange, pkce or code flow".into(),
        });
    }
    let override_code = config.flow != Flow::Exchange;
    if override_code && ex.grant_type.as_deref() == Some("") {
        errors.push(ValidationError {
            field: "exchange.grant_type".into(),
            message: "must not be empty".into(),
        });
    }
    if override_code && ex.grant_type.is_none() && ex.body.is_empty() {
        errors.push(ValidationError {
            field: "exchange.body".into(),
            message: "must not be empty without grant_type".into(),
        });
    }
    if let Some(field) = &ex.token_field {
        if safe_field_name(field).is_none() {
            errors.push(ValidationError {
                field: "exchange.token_field".into(),
                message: "must be 1-64 letters, digits, _, . or -".into(),
            });
        }
    }
}

fn reject_unavailable_capture_builtins(config: &OAuthConfig, errors: &mut Vec<ValidationError>) {
    let Some(url) = config.capture.url.as_deref() else {
        return;
    };
    let mut challenge = false;
    let mut secret = false;
    let mut rest = url;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else {
            break;
        };
        let name = &rest[..end];
        rest = &rest[end + 1..];
        if name == "code_challenge" && config.flow != Flow::Pkce && !challenge {
            challenge = true;
            errors.push(ValidationError {
                field: "capture.url".into(),
                message: "placeholder {code_challenge} is only available for pkce".into(),
            });
        }
        let missing_secret = config.client_secret.as_deref().is_none_or(str::is_empty);
        if name == "client_secret" && missing_secret && !secret {
            secret = true;
            errors.push(ValidationError {
                field: "capture.url".into(),
                message: "placeholder {client_secret} needs client_secret".into(),
            });
        }
    }
}

fn validate_capture_extra(config: &OAuthConfig, errors: &mut Vec<ValidationError>) {
    if config.capture.stage.is_some() && config.capture.at != CaptureAt::Network {
        errors.push(ValidationError {
            field: "capture.stage".into(),
            message: "only applies when capture.at is network".into(),
        });
    } else if config.capture.stage.is_some() && config.capture.from != CaptureFrom::Body {
        errors.push(ValidationError {
            field: "capture.stage".into(),
            message: "only applies to a network body capture".into(),
        });
    }
    if config.capture.fields.is_empty() {
        return;
    }
    let network_value = config.capture.at == CaptureAt::Network
        && matches!(config.capture.from, CaptureFrom::Header | CaptureFrom::Body);
    if !network_value {
        errors.push(ValidationError {
            field: "capture.fields".into(),
            message: "only applies to a network header or body capture".into(),
        });
        return;
    }
    for (name, spec) in &config.capture.fields {
        if !valid_env_name(name) {
            errors.push(ValidationError {
                field: format!("capture.fields.{name}"),
                message: "is not a valid environment variable name".into(),
            });
        }
        if !valid_field_spec(spec) {
            errors.push(ValidationError {
                field: format!("capture.fields.{name}"),
                message: "must be cookie:<name>, header:<name>, body:<name> or query:<name>".into(),
            });
        }
    }
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn valid_field_spec(spec: &str) -> bool {
    let Some((kind, rest)) = spec.split_once(':') else {
        return false;
    };
    matches!(kind, "cookie" | "header" | "body" | "query")
        && !rest.is_empty()
        && !rest.contains(':')
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

pub fn safe_field_name(name: &str) -> Option<&str> {
    let ok = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-');
    ok.then_some(name)
}

pub fn expects_state(authorize_url: &str) -> bool {
    authorize_url.contains("{state}")
}
