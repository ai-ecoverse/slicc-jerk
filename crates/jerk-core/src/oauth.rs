#[cfg(not(target_os = "wasi"))]
pub mod loopback;

#[cfg(not(target_os = "wasi"))]
pub mod device;

#[cfg(not(target_os = "wasi"))]
pub mod http_client;

#[cfg(not(target_os = "wasi"))]
pub mod intercept;

use crate::browser::Browser;
use crate::exit::Exit;
use crate::grants::Grants;
use jerk_oauth::config::OAuthConfig;
use std::collections::BTreeMap;
#[allow(unused_imports)]
use std::io::Write;

#[cfg(not(target_os = "wasi"))]
pub struct CdpResult(pub(crate) cdp_client::cdp::Cdp);

#[cfg(target_os = "wasi")]
pub enum CdpResult {}

pub trait CdpOpener {
    fn open(&self) -> Result<CdpResult, OAuthError>;
}

pub struct HttpPost<'a> {
    pub form: &'a dyn Fn(&str, &str) -> Result<String, String>,
    pub json: &'a dyn Fn(&str, &str) -> Result<String, String>,
}

pub struct OAuthEnv<'a> {
    pub config: &'a OAuthConfig,
    pub scope: Option<&'a str>,
    pub browser: &'a dyn Browser,
    pub grants: &'a dyn Grants,
    pub now_secs: u64,
    pub random_32: [u8; 32],
    pub random_16: [u8; 16],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OAuthError {
    pub exit: Exit,
    pub message: String,
}

impl std::fmt::Display for OAuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

pub trait OAuthHost: Send + Sync {
    fn browser(&self) -> &dyn Browser;
    fn grants(&self, store: Option<&str>) -> Box<dyn Grants>;
    fn http_post(&self, url: &str, body: &str) -> Result<String, String>;
    fn http_get(&self, url: &str) -> Result<String, String>;
    fn http_post_json(&self, url: &str, body: &str) -> Result<String, String>;
    fn mcp_probe(&self, url: &str, body: &str) -> McpProbeResult;
    fn sleep(&self, secs: u64);
    fn confine(&self, reach: crate::reach::Reach) -> Box<dyn OAuthHost>;
    fn reachable(&self, url: &str) -> Result<(), String>;
}

pub struct McpProbeResult {
    pub status: u16,
    pub www_authenticate: Option<String>,
    pub body: String,
}

#[cfg(not(target_os = "wasi"))]
pub mod default_host;

#[cfg(not(target_os = "wasi"))]
pub use default_host::DefaultOAuthHost;

#[cfg(target_os = "wasi")]
pub struct NoOAuthHost;

#[cfg(target_os = "wasi")]
impl OAuthHost for NoOAuthHost {
    fn browser(&self) -> &dyn Browser {
        &crate::browser::NoBrowser
    }
    fn grants(&self, _: Option<&str>) -> Box<dyn Grants> {
        Box::new(crate::grants::MemoryStore::new())
    }
    fn http_post(&self, _: &str, _: &str) -> Result<String, String> {
        Err("not available".into())
    }
    fn http_get(&self, _: &str) -> Result<String, String> {
        Err("not available".into())
    }
    fn http_post_json(&self, _: &str, _: &str) -> Result<String, String> {
        Err("not available".into())
    }
    fn mcp_probe(&self, _: &str, _: &str) -> McpProbeResult {
        McpProbeResult {
            status: 0,
            www_authenticate: None,
            body: String::new(),
        }
    }
    fn sleep(&self, _: u64) {}
    fn confine(&self, _: crate::reach::Reach) -> Box<dyn OAuthHost> {
        Box::new(NoOAuthHost)
    }
    fn reachable(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

pub(crate) fn accept_blocking(listener: &std::net::TcpListener) -> Option<std::net::TcpStream> {
    let (stream, _) = listener.accept().ok()?;
    stream.set_nonblocking(false).ok()?;
    Some(stream)
}

fn validate_config(config: &OAuthConfig) -> Result<(), OAuthError> {
    let errors = jerk_oauth::config::validate(config);
    if errors.is_empty() {
        Ok(())
    } else {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        Err(OAuthError {
            exit: Exit::Usage,
            message: format!("invalid oauth config: {}", msgs.join("; ")),
        })
    }
}

pub(crate) fn token_from_capture(
    config: &OAuthConfig,
    captured: &jerk_oauth::capture::CaptureResult,
    verifier: Option<&str>,
    scope: Option<&str>,
    now_secs: u64,
    http_post: &HttpPost<'_>,
) -> Result<Token, OAuthError> {
    use jerk_oauth::config::Flow;
    use jerk_oauth::expiry;

    match config.flow {
        Flow::Pkce | Flow::Code if config.exchange.is_some() => configured_exchange(
            config,
            &captured.value,
            verifier,
            scope,
            now_secs,
            http_post,
        ),
        Flow::Pkce | Flow::Code => exchange_authorization_code(
            config,
            &captured.value,
            &config.redirect_uri,
            verifier,
            now_secs,
            http_post.form,
        ),
        Flow::Exchange => {
            configured_exchange(config, &captured.value, None, scope, now_secs, http_post)
        }
        Flow::Implicit => {
            let expires_in = captured
                .params
                .get(config.expires_param.as_deref().unwrap_or("expires_in"))
                .and_then(|v| v.parse::<u64>().ok());
            let expires_at = expiry::compute_expiry(now_secs, expires_in, None)
                .or_else(|| crate::extract::jwt_expiry(&captured.value));
            Ok(Token {
                access_token: captured.value.clone(),
                refresh_token: None,
                expires_at,
                scope: captured.params.get("scope").cloned(),
                token_type: captured.params.get("token_type").cloned(),
                fields: BTreeMap::new(),
            })
        }
        Flow::ClientCredentials => Err(OAuthError {
            exit: Exit::Usage,
            message: "client_credentials does not use a capture".into(),
        }),
    }
}

fn configured_exchange(
    config: &OAuthConfig,
    value: &str,
    verifier: Option<&str>,
    scope: Option<&str>,
    now_secs: u64,
    http_post: &HttpPost<'_>,
) -> Result<Token, OAuthError> {
    use jerk_oauth::config::ExchangeFormat;
    use jerk_oauth::exchange;

    let exchange_cfg = config.exchange.as_ref().ok_or_else(|| OAuthError {
        exit: Exit::Usage,
        message: "exchange required for exchange flow".into(),
    })?;
    let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
        exit: Exit::Usage,
        message: "token_url required for exchange flow".into(),
    })?;
    let actual_scope = scope.or(config.scope.as_deref());
    let pairs = exchange::build_configured_exchange_body(
        exchange_cfg,
        value,
        verifier,
        &config.client_id,
        &config.redirect_uri,
        actual_scope,
    );
    let body = exchange::encode_exchange_body(exchange_cfg.format, &pairs);
    let post = match exchange_cfg.format {
        ExchangeFormat::Form => http_post.form,
        ExchangeFormat::Json => http_post.json,
    };
    let response = post(token_url, &body).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e,
    })?;
    let token_response =
        exchange::parse_token_response_field(&response, exchange_cfg.token_field.as_deref())
            .map_err(|e| OAuthError {
                exit: Exit::Error,
                message: e.to_string(),
            })?;
    let expires_at = jerk_oauth::expiry::compute_expiry(now_secs, token_response.expires_in, None);
    Ok(Token {
        access_token: token_response.access_token.clone(),
        refresh_token: token_response.refresh_token.clone(),
        expires_at,
        scope: token_response.scope.clone(),
        token_type: Some(token_response.token_type.clone()),
        fields: BTreeMap::new(),
    })
}

pub fn oauth_create(
    config: &OAuthConfig,
    scope: Option<&str>,
    host: &dyn OAuthHost,
    now_secs: u64,
    random: &dyn Fn(&mut [u8]),
    err: &mut dyn std::io::Write,
    cdp: Option<&dyn CdpOpener>,
) -> Result<Token, OAuthError> {
    oauth_create_with_vars(config, scope, host, now_secs, random, err, cdp, &[], &[])
}

#[allow(clippy::too_many_arguments)]
pub fn oauth_create_with_vars(
    config: &OAuthConfig,
    scope: Option<&str>,
    host: &dyn OAuthHost,
    now_secs: u64,
    random: &dyn Fn(&mut [u8]),
    err: &mut dyn std::io::Write,
    cdp: Option<&dyn CdpOpener>,
    extra_vars: &[(&str, &str)],
    cors_origins: &[String],
) -> Result<Token, OAuthError> {
    use jerk_oauth::config::Mode;

    enum InteractiveMode {
        Loopback,
        Auto,
        Device,
        Intercept,
        Popup,
    }

    validate_config(config)?;

    if config.flow == jerk_oauth::config::Flow::ClientCredentials
        && config.mode != Mode::ClientCredentials
    {
        let _ = (random, err, cdp, cors_origins);
        return oauth_client_credentials(config, scope, host, now_secs, extra_vars);
    }

    let mode = match config.mode {
        Mode::ClientCredentials => {
            let _ = (random, err, cdp, cors_origins);
            return oauth_client_credentials(config, scope, host, now_secs, extra_vars);
        }
        Mode::Loopback => InteractiveMode::Loopback,
        Mode::Auto => InteractiveMode::Auto,
        Mode::Device => InteractiveMode::Device,
        Mode::Intercept => InteractiveMode::Intercept,
        Mode::Popup => InteractiveMode::Popup,
    };

    let mut r32 = [0u8; 32];
    let mut r16 = [0u8; 16];
    random(&mut r32);
    random(&mut r16);

    jerk_oauth::config::validate_entropy(&r32, &r16).map_err(|msg| OAuthError {
        exit: Exit::Error,
        message: msg.into(),
    })?;

    #[cfg(not(target_os = "wasi"))]
    let post = HttpPost {
        form: &|url, body| host.http_post(url, body),
        json: &|url, body| host.http_post_json(url, body),
    };

    match mode {
        InteractiveMode::Loopback => {
            #[cfg(not(target_os = "wasi"))]
            {
                loopback::run(
                    config,
                    scope,
                    host.browser(),
                    now_secs,
                    &r32,
                    &r16,
                    &post,
                    extra_vars,
                    cors_origins,
                )
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err, r32, r16, extra_vars, cors_origins);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "loopback is not available in this environment".into(),
                })
            }
        }
        InteractiveMode::Auto => {
            #[cfg(not(target_os = "wasi"))]
            {
                let parsed = url::Url::parse(&config.redirect_uri).ok();
                let port = parsed.as_ref().and_then(|u| u.port());
                if let Some(p) = port {
                    if loopback::try_bind(p) {
                        return loopback::run(
                            config,
                            scope,
                            host.browser(),
                            now_secs,
                            &r32,
                            &r16,
                            &post,
                            extra_vars,
                            cors_origins,
                        );
                    }
                }
                let config = with_extra_placeholders(config, extra_vars)?;
                let opener = cdp.ok_or(OAuthError {
                    exit: Exit::Unavailable,
                    message: "cannot bind the loopback port and no CDP browser available; pass --cdp or run slicc launch".into(),
                })?;
                let CdpResult(mut cdp_conn) = opener.open()?;
                intercept::run(
                    &mut cdp_conn,
                    &config,
                    scope,
                    now_secs,
                    &r32,
                    &r16,
                    &post,
                    err,
                )
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err, r32, r16, extra_vars, cors_origins);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "auto mode is not available in this environment".into(),
                })
            }
        }
        InteractiveMode::Device => {
            #[cfg(not(target_os = "wasi"))]
            {
                oauth_device_create(config, scope, host, now_secs, err)
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "device flow is not available in this environment".into(),
                })
            }
        }
        InteractiveMode::Intercept => {
            #[cfg(not(target_os = "wasi"))]
            {
                let config = with_extra_placeholders(config, extra_vars)?;
                let opener = cdp.ok_or(OAuthError {
                    exit: Exit::Unavailable,
                    message: "intercept mode needs a CDP browser; pass --cdp or run slicc launch"
                        .into(),
                })?;
                let CdpResult(mut cdp_conn) = opener.open()?;
                intercept::run(
                    &mut cdp_conn,
                    &config,
                    scope,
                    now_secs,
                    &r32,
                    &r16,
                    &post,
                    err,
                )
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err, r32, r16);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "intercept mode is not available in this environment".into(),
                })
            }
        }
        InteractiveMode::Popup => Err(OAuthError {
            exit: Exit::Unsupported,
            message: "popup mode is not implemented yet".into(),
        }),
    }
}

pub(crate) fn with_extra_placeholders(
    config: &OAuthConfig,
    extra_vars: &[(&str, &str)],
) -> Result<OAuthConfig, OAuthError> {
    let mut next = config.clone();
    next.authorize_url = subst_extra(&config.authorize_url, extra_vars)?;
    if let Some(url) = &config.capture.url {
        next.capture.url = Some(subst_extra(url, extra_vars)?);
    }
    Ok(next)
}

fn subst_extra(template: &str, extra_vars: &[(&str, &str)]) -> Result<String, OAuthError> {
    let names = jerk_oauth::template::extra_placeholders(template);
    let mut pairs = Vec::new();
    for name in &names {
        let value = extra_vars
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| *value)
            .unwrap_or("");
        if value.is_empty() {
            return Err(OAuthError {
                exit: Exit::Usage,
                message: format!("pass -p {name}=… (required for this provider's login URL)"),
            });
        }
        pairs.push((name.as_str(), value));
    }
    Ok(jerk_oauth::template::render(template, &pairs))
}

pub fn oauth_client_credentials(
    config: &OAuthConfig,
    scope: Option<&str>,
    host: &dyn OAuthHost,
    now_secs: u64,
    extra_vars: &[(&str, &str)],
) -> Result<Token, OAuthError> {
    validate_config(config)?;
    let client_id = jerk_oauth::template::render_plain(&config.client_id, extra_vars);
    let secret_tmpl = config.client_secret.as_deref().unwrap_or("");
    let client_secret = jerk_oauth::template::render_plain(secret_tmpl, extra_vars);
    let scope_tmpl = scope
        .map(str::to_string)
        .or_else(|| {
            config
                .scope
                .as_ref()
                .map(|s| jerk_oauth::template::render_plain(s, extra_vars))
        })
        .filter(|s| !s.is_empty());
    if client_id.is_empty() || client_id.contains('{') {
        return Err(OAuthError {
            exit: Exit::Usage,
            message: "client_id is required (-p client_id=… or params.client_id)".into(),
        });
    }
    if client_secret.is_empty() || client_secret.contains('{') {
        return Err(OAuthError {
            exit: Exit::Usage,
            message: "client_secret is required (-p client_secret=… or params.client_secret)"
                .into(),
        });
    }
    let token_url_tmpl = config.token_url.as_deref().unwrap_or("");
    let token_url = jerk_oauth::template::render_plain(token_url_tmpl, extra_vars);
    if token_url.is_empty() || token_url.contains('{') {
        return Err(OAuthError {
            exit: Exit::Usage,
            message: "token_url required for client_credentials".into(),
        });
    }
    let (url, form_body) = jerk_oauth::exchange::client_credentials_request(
        &jerk_oauth::exchange::ClientCredentialsParams {
            token_url: &token_url,
            client_id: &client_id,
            client_secret: &client_secret,
            scope: scope_tmpl.as_deref(),
        },
    )
    .map_err(|e| OAuthError {
        exit: Exit::Usage,
        message: e,
    })?;
    let response = host.http_post(&url, &form_body).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e,
    })?;
    let parsed = jerk_oauth::exchange::parse_token_response(&response).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e.to_string(),
    })?;
    let expires_at = jerk_oauth::expiry::compute_expiry(now_secs, parsed.expires_in, None);
    Ok(Token {
        access_token: parsed.access_token.clone(),
        refresh_token: parsed.refresh_token.clone(),
        expires_at,
        scope: parsed.scope.clone().or(scope_tmpl),
        token_type: Some(parsed.token_type.clone()),
        fields: BTreeMap::new(),
    })
}

#[cfg(not(target_os = "wasi"))]
fn oauth_device_create(
    config: &OAuthConfig,
    scope: Option<&str>,
    host: &dyn OAuthHost,
    now_secs: u64,
    err: &mut dyn std::io::Write,
) -> Result<Token, OAuthError> {
    let actual_scope = scope.or(config.scope.as_deref()).unwrap_or("");
    let body = format!(
        "client_id={}&scope={}",
        url::form_urlencoded::byte_serialize(config.client_id.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(actual_scope.as_bytes()).collect::<String>(),
    );
    let response = host
        .http_post(&config.authorize_url, &body)
        .map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e,
        })?;
    let auth = device::parse_device_auth_response(&response)?;
    let _ = writeln!(
        err,
        "Visit {} and enter code: {}",
        auth.verification_uri, auth.user_code
    );
    let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
        exit: Exit::Usage,
        message: "token_url required for device flow".into(),
    })?;
    device::poll_token(&device::PollParams {
        token_url,
        device_code: &auth.device_code,
        client_id: &config.client_id,
        interval: auth.interval,
        expires_in: auth.expires_in,
        now_secs,
        http_post: &|url, body| host.http_post(url, body),
        sleep: &|secs| host.sleep(secs),
    })
}

pub(crate) fn exchange_authorization_code(
    config: &OAuthConfig,
    code: &str,
    redirect_uri: &str,
    verifier: Option<&str>,
    now_secs: u64,
    http_post: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Result<Token, OAuthError> {
    use jerk_oauth::config::Flow;
    use jerk_oauth::exchange;
    use jerk_oauth::expiry;

    let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
        exit: Exit::Usage,
        message: match config.flow {
            Flow::Pkce => "token_url required for pkce flow".into(),
            Flow::Code => "token_url required for code flow".into(),
            Flow::Exchange => "token_url required for exchange flow".into(),
            Flow::Implicit => "token_url required".into(),
            Flow::ClientCredentials => "token_url required for client_credentials".into(),
        },
    })?;
    let body = exchange::build_code_exchange_body(
        code,
        &config.client_id,
        redirect_uri,
        verifier,
        config.client_secret.as_deref(),
    );
    let form_body = exchange::encode_form_body(&body);
    let response = http_post(token_url, &form_body).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e,
    })?;
    let token_response = exchange::parse_token_response(&response).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e.to_string(),
    })?;
    let expires_at = expiry::compute_expiry(now_secs, token_response.expires_in, None);
    Ok(Token {
        access_token: token_response.access_token.clone(),
        refresh_token: token_response.refresh_token.clone(),
        expires_at,
        scope: token_response.scope.clone(),
        token_type: Some(token_response.token_type.clone()),
        fields: BTreeMap::new(),
    })
}

pub fn oauth_refresh(
    config: &OAuthConfig,
    refresh_token: &str,
    host: &dyn OAuthHost,
    now_secs: u64,
) -> Result<Token, OAuthError> {
    validate_config(config)?;

    let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
        exit: Exit::Error,
        message: "no token_url for refresh".into(),
    })?;
    let (url, form_body) =
        jerk_oauth::exchange::refresh_request(&jerk_oauth::exchange::RefreshParams {
            token_url,
            client_id: &config.client_id,
            refresh_token,
            scope: None,
            resource: None,
            client_secret: config.client_secret.as_deref(),
        })
        .map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e,
        })?;
    let response = host.http_post(&url, &form_body).map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e,
    })?;
    let parsed = jerk_oauth::exchange::parse_token_response(&response).map_err(|e| {
        let exit = match &e {
            jerk_oauth::exchange::ExchangeError::IdpError { error, .. }
                if error == "invalid_grant" =>
            {
                Exit::SignIn
            }
            _ => Exit::Error,
        };
        OAuthError {
            exit,
            message: e.to_string(),
        }
    })?;
    let expires_at = jerk_oauth::expiry::compute_expiry(now_secs, parsed.expires_in, None);
    Ok(Token {
        access_token: parsed.access_token.clone(),
        refresh_token: parsed.refresh_token.clone(),
        expires_at,
        scope: parsed.scope.clone(),
        token_type: Some(parsed.token_type.clone()),
        fields: BTreeMap::new(),
    })
}
