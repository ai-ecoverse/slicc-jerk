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
#[allow(unused_imports)]
use std::io::Write;

#[cfg(not(target_os = "wasi"))]
pub struct CdpResult(pub(crate) cdp_client::cdp::Cdp);

#[cfg(target_os = "wasi")]
pub enum CdpResult {}

pub trait CdpOpener {
    fn open(&self) -> Result<CdpResult, String>;
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
    fn sleep(&self, secs: u64);
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
    fn sleep(&self, _: u64) {}
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
    use jerk_oauth::config::Mode;

    let mut r32 = [0u8; 32];
    let mut r16 = [0u8; 16];
    random(&mut r32);
    random(&mut r16);

    match config.mode {
        Mode::Loopback => {
            #[cfg(not(target_os = "wasi"))]
            {
                loopback::run(
                    config,
                    scope,
                    host.browser(),
                    now_secs,
                    &r32,
                    &r16,
                    &|url, body| host.http_post(url, body),
                )
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err, r32, r16);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "loopback is not available in this environment".into(),
                })
            }
        }
        Mode::Auto => {
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
                            &|url, body| host.http_post(url, body),
                        );
                    }
                }
                let opener = cdp.ok_or(OAuthError {
                    exit: Exit::Unavailable,
                    message: "cannot bind the loopback port and no CDP browser available; pass --cdp or run slicc launch".into(),
                })?;
                let CdpResult(mut cdp_conn) = opener.open().map_err(|e| OAuthError {
                    exit: Exit::Unavailable,
                    message: e,
                })?;
                intercept::run(
                    &mut cdp_conn,
                    config,
                    scope,
                    now_secs,
                    &r32,
                    &r16,
                    &|url, body| host.http_post(url, body),
                    err,
                )
            }
            #[cfg(target_os = "wasi")]
            {
                let _ = (scope, err, r32, r16);
                Err(OAuthError {
                    exit: Exit::Unsupported,
                    message: "auto mode is not available in this environment".into(),
                })
            }
        }
        Mode::Device => {
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
        Mode::Intercept => {
            #[cfg(not(target_os = "wasi"))]
            {
                let opener = cdp.ok_or(OAuthError {
                    exit: Exit::Unavailable,
                    message: "intercept mode needs a CDP browser; pass --cdp or run slicc launch"
                        .into(),
                })?;
                let CdpResult(mut cdp_conn) = opener.open().map_err(|e| OAuthError {
                    exit: Exit::Unavailable,
                    message: e,
                })?;
                intercept::run(
                    &mut cdp_conn,
                    config,
                    scope,
                    now_secs,
                    &r32,
                    &r16,
                    &|url, body| host.http_post(url, body),
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
        Mode::Popup => Err(OAuthError {
            exit: Exit::Unsupported,
            message: "popup mode is not implemented yet".into(),
        }),
    }
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

pub fn oauth_refresh(
    config: &OAuthConfig,
    refresh_token: &str,
    host: &dyn OAuthHost,
    now_secs: u64,
) -> Result<Token, OAuthError> {
    let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
        exit: Exit::Error,
        message: "no token_url for refresh".into(),
    })?;
    let body = jerk_oauth::exchange::build_refresh_body(refresh_token, &config.client_id);
    let form_body = jerk_oauth::exchange::encode_form_body(&body);
    let response = host
        .http_post(token_url, &form_body)
        .map_err(|e| OAuthError {
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
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_at,
        scope: parsed.scope,
        token_type: parsed.token_type,
    })
}
