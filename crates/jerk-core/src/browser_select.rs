use crate::exit::Exit;
use crate::oauth::OAuthError;
use crate::paths::{Env, Os};
use crate::run::{Opts, Outcome, Runner, REFUSED};
use crate::which;
use cdp_client::connect::Start;
use cdp_client::discover;
use cdp_client::resolve::{self, Choice, Finder, Rule, Step};
use serde_json::Value;
use std::path::Path;

pub const JERK_NOTES: &str = "In jerk, a seven tab on --seven-origin comes first in the slicc browsers listing. jerk always prints \"* CDP <endpoint> (via <rule>)\" to stderr as soon as the endpoint is chosen, with no --verbose needed. When nothing is found, the error lists every step tried and why it was passed over. When a saved endpoint does not answer, the error names it. jerk tab in the kernel needs SLICC_CDP_URL.";

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub start: Start,
    pub rule: Rule,
}

impl Found {
    pub fn endpoint(&self) -> &str {
        resolve::endpoint(&self.start)
    }

    pub fn loopback(&self) -> bool {
        resolve::loopback_url(self.endpoint())
    }

    pub fn describe(&self) -> String {
        resolve::describe(&self.choice(), false)
            .trim_end()
            .to_string()
    }

    fn choice(&self) -> Choice {
        Choice {
            start: self.start.clone(),
            rule: self.rule,
        }
    }
}

impl From<Choice> for Found {
    fn from(choice: Choice) -> Found {
        Found {
            start: choice.start,
            rule: choice.rule,
        }
    }
}

pub struct Lookup<'a> {
    pub env: &'a Env,
    pub runner: &'a dyn Runner,
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub probe: &'a dyn Fn(&str) -> Result<String, String>,
    pub cwd: Option<std::path::PathBuf>,
}

impl<'a> Lookup<'a> {
    pub fn new(
        env: &'a Env,
        runner: &'a dyn Runner,
        exists: &'a dyn Fn(&Path) -> bool,
    ) -> Lookup<'a> {
        Lookup {
            env,
            runner,
            exists,
            probe: &http_body,
            cwd: std::env::current_dir().ok(),
        }
    }
}

fn usage(message: String) -> OAuthError {
    OAuthError {
        exit: Exit::Usage,
        message: message.trim_end().to_string(),
    }
}

pub fn explicit(flag: Option<&str>, env: &Env) -> Result<Option<Found>, OAuthError> {
    resolve::explicit_start(flag, env.var("SLICC_CDP_URL"))
        .transpose()
        .map(|choice| choice.map(Found::from))
        .map_err(usage)
}

struct JerkFinder<'a, 'b> {
    lookup: &'b Lookup<'a>,
    seven_origin: Option<&'b str>,
    refused: Option<OAuthError>,
}

impl Finder for JerkFinder<'_, '_> {
    fn attached(&mut self, trail: &mut Vec<Step>) -> Result<Option<(Rule, String)>, String> {
        match from_slicc(self.seven_origin, self.lookup, trail) {
            Ok(Some(url)) => return Ok(Some((Rule::SliccBrowsers, url))),
            Ok(None) => {}
            Err(refused) => {
                self.refused = Some(refused);
                return Err(REFUSED.to_string());
            }
        }
        Ok(from_active_port(self.lookup, trail).map(|url| (Rule::ActivePort, url)))
    }

    fn saved(&mut self, trail: &mut Vec<Step>) -> Option<String> {
        let text = session_file(self.lookup).and_then(|file| std::fs::read_to_string(file).ok());
        let mut probe = |url: &str| (self.lookup.probe)(url);
        resolve::saved_from_json(text.as_deref(), &mut probe, trail)
    }

    fn probe(&mut self, trail: &mut Vec<Step>) -> Option<String> {
        let ports = self
            .lookup
            .env
            .var(cdp_client::ports::OVERRIDE)
            .and_then(cdp_client::ports::parse)
            .unwrap_or_else(cdp_client::ports::catalog);
        let mut probe = |url: &str| (self.lookup.probe)(url);
        resolve::probe_ports(&ports, &mut probe, trail)
    }
}

pub fn resolve(
    flag: Option<&str>,
    seven_origin: Option<&str>,
    lookup: &Lookup<'_>,
) -> Result<Found, OAuthError> {
    let mut finder = JerkFinder {
        lookup,
        seven_origin,
        refused: None,
    };
    let resolved = resolve::resolve(flag, lookup.env.var("SLICC_CDP_URL"), &mut finder);
    if let Some(refused) = finder.refused {
        return Err(refused);
    }
    let resolved = resolved.map_err(usage)?;
    match resolved.choice {
        Some(choice) => Ok(choice.into()),
        None => Err(OAuthError {
            exit: Exit::Unavailable,
            message: resolve::not_found("jerk", &resolved.trail)
                .trim_start_matches("jerk: ")
                .trim_end()
                .to_string(),
        }),
    }
}

fn from_slicc(
    seven_origin: Option<&str>,
    lookup: &Lookup<'_>,
    trail: &mut Vec<Step>,
) -> Result<Option<String>, OAuthError> {
    let failed = |trail: &mut Vec<Step>| {
        trail.push(Step::new(
            Rule::SliccBrowsers,
            "slicc not on PATH, or it failed",
        ));
        Ok(None)
    };
    let Some(stdout) = slicc_listing("browsers", lookup)? else {
        return failed(trail);
    };
    let Some(entries) = listed_entries(&stdout) else {
        return failed(trail);
    };
    match pick_listed(&entries, seven_origin) {
        Some(url) => Ok(Some(url)),
        None => {
            trail.push(Step::new(
                Rule::SliccBrowsers,
                "no loopback CDP endpoint listed",
            ));
            Ok(None)
        }
    }
}

fn slicc_listing(command: &str, lookup: &Lookup<'_>) -> Result<Option<String>, OAuthError> {
    let Some(slicc) = which::find("slicc", lookup.env, lookup.exists) else {
        return Ok(None);
    };
    match lookup.runner.run(
        &slicc,
        &[command.to_string(), "--json".to_string()],
        &Opts {
            interactive: false,
            capture: true,
            echo: false,
            timeout: Some(discover::CLI_TIMEOUT),
            remove_env: &[],
            input: None,
            on_stderr: None,
        },
    ) {
        Outcome::Refused => Err(OAuthError {
            exit: Exit::Refused,
            message: REFUSED.into(),
        }),
        Outcome::Exited {
            code: 0, stdout, ..
        } => Ok(Some(stdout)),
        _ => Ok(None),
    }
}

pub(crate) fn remote_callback(
    found: &Found,
    lookup: &Lookup<'_>,
) -> Result<Option<&'static str>, OAuthError> {
    if !found.loopback() {
        return Ok(Some("remote browser"));
    }
    let port = url::Url::parse(found.endpoint())
        .ok()
        .and_then(|url| url.port_or_known_default());
    let relay = slicc_listing("tray", lookup)?
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row["mode"] == "cdp"
                    && row["state"] == "connected"
                    && port.is_some_and(|p| row["cdp"]["port"].as_u64() == Some(u64::from(p)))
            })
        });
    Ok(relay.then_some("browser behind a SLICC relay"))
}

fn listed_entries(text: &str) -> Option<Vec<Value>> {
    let value: Value = serde_json::from_str(text).ok()?;
    if value.get("schema").and_then(Value::as_u64) != Some(1) {
        return None;
    }
    value.get("entries")?.as_array().cloned()
}

fn pick_listed(entries: &[Value], seven_origin: Option<&str>) -> Option<String> {
    if let Some(origin) = seven_origin.map(|o| o.trim_end_matches('/')) {
        let on_origin: Vec<Value> = entries
            .iter()
            .filter(|entry| has_seven_tab(entry, origin))
            .cloned()
            .collect();
        if let Some(url) = discover::pick_cli_endpoint(&on_origin) {
            return Some(url);
        }
    }
    discover::pick_cli_endpoint(entries)
}

fn has_seven_tab(entry: &Value, origin: &str) -> bool {
    entry
        .pointer("/endpoint/sevenTabs")
        .and_then(Value::as_array)
        .is_some_and(|tabs| {
            tabs.iter().any(|tab| {
                let url = tab
                    .get("url")
                    .and_then(Value::as_str)
                    .or_else(|| tab.as_str())
                    .unwrap_or("");
                url == origin || url.starts_with(&format!("{origin}/"))
            })
        })
}

fn host_env(env: &Env) -> discover::HostEnv {
    let owned = |name: &str| env.var(name).map(str::to_string);
    discover::HostEnv {
        platform: match env.os {
            Os::MacOs => "macos",
            Os::Windows => "windows",
            Os::Unix => "linux",
            Os::Wasi => "other",
        },
        home: env.home_dir().to_string_lossy().into_owned(),
        app_data: owned("APPDATA"),
        local_app_data: owned("LOCALAPPDATA"),
        xdg_config_home: owned("XDG_CONFIG_HOME"),
        xdg_state_home: owned("XDG_STATE_HOME"),
    }
}

struct DiskFs;

impl discover::ScanFs for DiskFs {
    fn list_dirs(&self, path: &str) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        names.sort();
        names
    }

    fn read_file(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        let micros = modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_micros();
        i64::try_from(micros).ok()
    }
}

fn from_active_port(lookup: &Lookup<'_>, trail: &mut Vec<Step>) -> Option<String> {
    let env = host_env(lookup.env);
    let roots = discover::slicc_profile_roots(&env);
    let mut probe = |url: &str| (lookup.probe)(url);
    let note = match discover::from_devtools_active_port(&env, &DiskFs, &roots, &mut probe) {
        Ok(Some(url)) => return Some(url),
        Ok(None) => {
            "no live file under slicc seven-profiles, profiles, or six profiles".to_string()
        }
        Err(message) => message.trim_end().to_string(),
    };
    trail.push(Step::new(Rule::ActivePort, note));
    None
}

fn config_dir(env: &Env) -> std::path::PathBuf {
    match env.os {
        Os::MacOs => env.home_dir().join("Library").join("Application Support"),
        Os::Windows => env
            .var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env.home_dir().join("AppData").join("Roaming")),
        _ => env
            .var("XDG_CONFIG_HOME")
            .filter(|dir| dir.starts_with('/'))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env.home_dir().join(".config")),
    }
}

fn session_file(lookup: &Lookup<'_>) -> Option<std::path::PathBuf> {
    let cwd = lookup.cwd.as_ref()?;
    let canonical = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
    let key = cdp_client::hash::sha256_hex(canonical.as_os_str().as_encoded_bytes());
    Some(
        config_dir(lookup.env)
            .join("slicc")
            .join("playwright-cli")
            .join("sessions")
            .join(format!("{key}.json")),
    )
}

pub fn http_body(url: &str) -> Result<String, String> {
    let response = cdp_client::net::http_get(url)?;
    Ok(String::from_utf8_lossy(&response.body).into_owned())
}

pub fn connect_with<T>(
    found: &Found,
    dial: &dyn Fn(Start) -> Result<T, String>,
) -> Result<T, String> {
    dial(found.start.clone()).map_err(|message| {
        let hint = resolve::saved_hint("jerk", &found.choice(), &message);
        if hint.is_empty() {
            message
        } else {
            format!("{}\n{}", message.trim_end(), hint.trim_end())
        }
    })
}

pub const SIGN_IN: &str = "A loopback sign-in (create) opens its page in that browser when --cdp or SLICC_CDP_URL is given, or when stdin is not a terminal and a browser is found; otherwise in the system browser. jerk says on stderr where the page opened and how long it waits. For a remote CDP browser or a connected slicc cdp relay, jerk intercepts the callback in the selected browser instead of listening locally.";

pub fn help() -> String {
    let mut out = String::from("\nCDP browser (create, intercept, tab):\n");
    let order = resolve::CONNECT_ORDER.replace("curlwright reads it", "jerk reads it");
    for text in [order.as_str(), resolve::CDP_FORMS, JERK_NOTES, SIGN_IN] {
        let mut line = String::from(" ");
        for word in text.split(' ') {
            if line.len() + word.len() + 1 > 80 {
                out.push_str(&line);
                out.push('\n');
                line = String::from(" ");
            }
            line.push(' ');
            line.push_str(word);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}
