#[cfg(not(target_os = "wasi"))]
pub mod loopback;

#[cfg(not(target_os = "wasi"))]
pub mod device;

#[cfg(not(target_os = "wasi"))]
pub mod http_client;

use crate::browser::Browser;
use crate::exit::Exit;
use crate::grants::Grants;
use jerk_oauth::config::OAuthConfig;

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
