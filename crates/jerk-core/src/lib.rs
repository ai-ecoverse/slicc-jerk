pub mod browser;
#[cfg(not(target_os = "wasi"))]
pub mod browser_select;
pub mod builtin;
pub mod cli;
pub mod config;
pub mod dispatch;
pub mod exit;
pub mod extract;
pub mod fnox;
pub mod grants;
#[cfg(not(target_os = "wasi"))]
pub mod intercept_cmd;
pub mod kernel;
pub mod link;
pub mod mask;
pub mod mcp;
pub mod mcp_config;
pub mod oauth;
pub mod output;
pub mod paste;
pub mod paths;
pub mod reach;
pub mod run;
pub mod sealed;
pub mod source;
pub mod state;
pub mod tab;
pub mod template;
pub mod time;
pub mod upgrade;
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

#[cfg(all(test, jerk_unit))]
mod mcp_unit {
    include!("../../../test/unit/mcp_tests.rs");
}

#[cfg(all(test, jerk_unit))]
mod tab_unit {
    include!("../../../test/unit/tab_tests.rs");
}
