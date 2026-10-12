pub mod backoff;
pub mod capture;
pub mod config;
pub mod exchange;
pub mod expiry;
pub mod pkce;
pub mod relay;
pub mod rewrite;
pub mod scopes;
pub mod template;

#[cfg(all(test, jerk_unit))]
mod unit {
    include!("../../../test/unit/jerk_oauth.rs");
}
