pub mod cli;
pub mod exit;

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
