pub mod backoff;
pub mod capture;
pub mod config;
pub mod discovery;
pub mod exchange;
pub mod expiry;
pub mod pkce;
pub mod registration;
pub mod relay;
pub mod rewrite;
pub mod scopes;
pub mod template;
pub mod www_authenticate;

#[cfg(all(test, jerk_unit))]
mod unit {
    include!("../../../test/unit/jerk_oauth.rs");
}
