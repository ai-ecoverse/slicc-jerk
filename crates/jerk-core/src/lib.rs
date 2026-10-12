pub mod browser;
pub mod builtin;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod exit;
pub mod extract;
pub mod fnox;
pub mod grants;
pub mod mask;
pub mod oauth;
pub mod output;
pub mod paths;
pub mod run;
pub mod scopes;
pub mod source;
pub mod state;
pub mod template;
pub mod time;
pub mod which;

pub fn version() -> &'static str {
    pick_version(option_env!("JERK_VERSION"))
}

pub(crate) fn pick_version(built: Option<&'static str>) -> &'static str {
    match built {
        Some(v) if !v.is_empty() => v,
        _ => env!("CARGO_PKG_VERSION"),
    }
}

#[cfg(all(test, jerk_unit))]
mod unit {
    include!("../../../test/unit/jerk_core.rs");
}
