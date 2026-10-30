use crate::exit::Exit;
use crate::oauth::OAuthError;
use crate::paths::Env;
use crate::run::{Opts, Outcome, Runner, REFUSED};
use crate::which;
use std::io::Write;
use std::path::Path;

pub fn resolve_cdp_url(
    explicit: Option<&str>,
    seven_origin: Option<&str>,
    env: &Env,
    runner: &dyn Runner,
    exists: &dyn Fn(&Path) -> bool,
    err: &mut dyn Write,
) -> Result<String, OAuthError> {
    if let Some(url) = explicit {
        return Ok(url.to_string());
    }

    if let Some(url) = env.var("SLICC_CDP_URL") {
        return Ok(url.to_string());
    }

    if let Some(slicc_path) = which::find("slicc", env, exists) {
        let outcome = runner.run(
            &slicc_path,
            &["browsers".to_string(), "--json".to_string()],
            &Opts {
                interactive: false,
                capture: true,
                echo: false,
                timeout: Some(std::time::Duration::from_secs(5)),
                remove_env: &[],
                input: None,
                on_stderr: None,
            },
        );
        if outcome == Outcome::Refused {
            return Err(OAuthError {
                exit: Exit::Refused,
                message: REFUSED.into(),
            });
        }
        if let Outcome::Exited {
            code: 0, stdout, ..
        } = outcome
        {
            if let Some(url) = pick_browser(&stdout, seven_origin, err) {
                return Ok(url);
            }
        }
    }

    Err(OAuthError {
        exit: Exit::Unavailable,
        message: "no CDP browser found; pass --cdp, set SLICC_CDP_URL, or run slicc launch".into(),
    })
}

#[derive(serde::Deserialize)]
struct BrowserEntry {
    endpoint: String,
    #[serde(default)]
    browser: Option<String>,
    #[serde(default, rename = "sevenOpen")]
    seven_open: bool,
    #[serde(default, rename = "sevenTabs")]
    seven_tabs: Vec<String>,
}

fn pick_browser(
    json_text: &str,
    seven_origin: Option<&str>,
    err: &mut dyn Write,
) -> Option<String> {
    let entries: Vec<BrowserEntry> = serde_json::from_str(json_text).ok()?;
    if entries.is_empty() {
        return None;
    }

    if let Some(entry) = entries.iter().find(|e| e.seven_open) {
        if let Some(origin) = seven_origin {
            if entry.seven_tabs.iter().any(|tab| tab == origin) {
                return Some(entry.endpoint.clone());
            }
        } else {
            return Some(entry.endpoint.clone());
        }
    }

    if let Some(origin) = seven_origin {
        let matches: Vec<&BrowserEntry> = entries
            .iter()
            .filter(|e| e.seven_tabs.iter().any(|tab| tab == origin))
            .collect();
        if matches.len() == 1 {
            return Some(matches[0].endpoint.clone());
        }
        if matches.len() > 1 {
            let _ = writeln!(err, "jerk: multiple browsers have {origin}:");
            for m in &matches {
                let label = m.browser.as_deref().unwrap_or("unknown");
                let _ = writeln!(err, "  {label}: {}", m.endpoint);
            }
            return None;
        }
    }

    let preferred = entries.iter().find(|e| {
        e.browser
            .as_deref()
            .is_some_and(|b| b.to_ascii_lowercase().contains("chrome"))
    });
    if let Some(entry) = preferred {
        return Some(entry.endpoint.clone());
    }

    Some(entries[0].endpoint.clone())
}
