// Ported from slicc-cdp crates/cdp-client/src/resolve.rs (cdp_client::resolve); switch to it once published.
use crate::exit::Exit;
use crate::oauth::OAuthError;
use crate::paths::{Env, Os};
use crate::run::{Opts, Outcome, Runner, REFUSED};
use crate::which;
use cdp_client::connect::{self, Start};
use cdp_client::discover;
use serde_json::Value;
use std::path::Path;

pub const AUTO: &str = "auto";

pub const CONNECT_ORDER: &str = "Browser, in order: --cdp wins; else SLICC_CDP_URL; else the slicc browsers JSON listing when slicc is on PATH (a seven tab on --seven-origin, then sevenOpen, then a slicc profile, then best tier; loopback ws only); else a live DevToolsActivePort under slicc seven-profiles, legacy profiles, and SLICC six profile dirs (loopback /json/version check, websocket pathname must match the file, newest live wins); else a loopback endpoint playwright-cli saved for this directory under the user config dir; else the first of 127.0.0.1:9222-9229 whose /json/version answers. Saved state never dials a non-loopback host. The kernel build uses SLICC_CDP_URL.";

pub const CDP_FORMS: &str = "--cdp and SLICC_CDP_URL take a port, host[:port], [ipv6][:port], ws://, wss://, http://, or https://. A ws:// or wss:// URL with a path is dialed directly. An http:// URL, a path-less ws:// URL, a port, or host[:port] is /json/version discovery. The debugger path is opaque. \"auto\" (or an empty value) skips that step, so --cdp auto ignores SLICC_CDP_URL.";

pub const CDP_REPORT: &str = "jerk prints \"* CDP <endpoint> (via <rule>)\" to stderr before connecting. When nothing is found, the error lists every step tried and why it was passed over.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Flag,
    Env,
    SliccBrowsers,
    ActivePort,
    Saved,
    Probe,
}

impl Rule {
    pub fn label(self) -> &'static str {
        match self {
            Rule::Flag => "--cdp",
            Rule::Env => "SLICC_CDP_URL",
            Rule::SliccBrowsers => "slicc browsers",
            Rule::ActivePort => "DevToolsActivePort",
            Rule::Saved => "saved endpoint",
            Rule::Probe => "port probe",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub start: Start,
    pub rule: Rule,
}

impl Found {
    pub fn endpoint(&self) -> &str {
        match &self.start {
            Start::Direct(url) | Start::Discover(url) => url,
        }
    }

    pub fn loopback(&self) -> bool {
        url::Url::parse(self.endpoint())
            .ok()
            .and_then(|url| url.host_str().map(loopback_host))
            .unwrap_or(false)
    }

    pub fn describe(&self) -> String {
        format!(
            "* CDP {} (via {})",
            cdp_client::sanitize::redact_url(self.endpoint()),
            self.rule.label()
        )
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

fn given(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty() && !text.eq_ignore_ascii_case(AUTO))
}

fn skips_env(flag: Option<&str>) -> bool {
    flag.map(str::trim)
        .is_some_and(|text| text.eq_ignore_ascii_case(AUTO))
}

pub fn unsupported(raw: &str) -> String {
    format!(
        "unsupported CDP URL \"{raw}\" (expected a port, host:port, ws://, wss://, http://, or https://; use --cdp auto to search for a running browser)"
    )
}

pub fn explicit(flag: Option<&str>, env: &Env) -> Result<Option<Found>, OAuthError> {
    let start = |raw: &str, rule: Rule| -> Result<Found, OAuthError> {
        let start = connect::choose_start(Some(raw), None).map_err(|_| OAuthError {
            exit: Exit::Usage,
            message: unsupported(raw),
        })?;
        Ok(Found { start, rule })
    };
    if let Some(raw) = given(flag) {
        return start(raw, Rule::Flag).map(Some);
    }
    if skips_env(flag) {
        return Ok(None);
    }
    match given(env.var("SLICC_CDP_URL")) {
        Some(raw) => start(raw, Rule::Env).map(Some),
        None => Ok(None),
    }
}

type Trail = Vec<(Rule, String)>;

pub fn resolve(
    flag: Option<&str>,
    seven_origin: Option<&str>,
    lookup: &Lookup<'_>,
) -> Result<Found, OAuthError> {
    if let Some(found) = explicit(flag, lookup.env)? {
        return Ok(found);
    }
    let flag_note = if skips_env(flag) { "auto" } else { "not given" };
    let env_note = if skips_env(flag) && given(lookup.env.var("SLICC_CDP_URL")).is_some() {
        "set, skipped by --cdp auto"
    } else {
        "not set"
    };
    let mut trail: Trail = vec![
        (Rule::Flag, flag_note.to_string()),
        (Rule::Env, env_note.to_string()),
    ];
    if let Some(url) = from_slicc(seven_origin, lookup, &mut trail)? {
        return Ok(discovered(url, Rule::SliccBrowsers));
    }
    let host = (!cfg!(target_os = "wasi"))
        .then(|| on_host(lookup, &mut trail))
        .flatten();
    host.ok_or_else(|| not_found(&trail))
}

fn on_host(lookup: &Lookup<'_>, trail: &mut Trail) -> Option<Found> {
    if let Some(url) = from_active_port(lookup, trail) {
        return Some(discovered(url, Rule::ActivePort));
    }
    if let Some(start) =
        from_saved(lookup, trail).and_then(|url| connect::choose_start(Some(&url), None).ok())
    {
        return Some(Found {
            start,
            rule: Rule::Saved,
        });
    }
    from_probe(lookup, trail).map(|url| discovered(url, Rule::Probe))
}

fn not_found(trail: &Trail) -> OAuthError {
    let mut message = String::from("no CDP browser found. Tried, in order:\n");
    for (rule, note) in trail {
        message.push_str(&format!("  {:<19} {note}\n", rule.label()));
    }
    message.push_str("Pass --cdp <port|host:port|ws://...|http://...>, set SLICC_CDP_URL, or start a browser with --remote-debugging-port.");
    OAuthError {
        exit: Exit::Unavailable,
        message,
    }
}

fn discovered(url: String, rule: Rule) -> Found {
    Found {
        start: Start::Discover(url),
        rule,
    }
}

fn from_slicc(
    seven_origin: Option<&str>,
    lookup: &Lookup<'_>,
    trail: &mut Trail,
) -> Result<Option<String>, OAuthError> {
    let failed = |trail: &mut Trail| {
        trail.push((
            Rule::SliccBrowsers,
            "slicc not on PATH, or it failed".to_string(),
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
            trail.push((
                Rule::SliccBrowsers,
                "no loopback CDP endpoint listed".to_string(),
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

fn from_active_port(lookup: &Lookup<'_>, trail: &mut Trail) -> Option<String> {
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
    trail.push((Rule::ActivePort, note));
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

fn known_provenance(text: &str) -> bool {
    matches!(text, "explicit" | "seven-profile" | "probe")
}

fn loopback_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|parsed| {
        matches!(parsed.scheme(), "http" | "https" | "ws" | "wss")
            && parsed.host_str().is_some_and(loopback_host)
    })
}

fn from_saved(lookup: &Lookup<'_>, trail: &mut Trail) -> Option<String> {
    let text = session_file(lookup).and_then(|file| std::fs::read_to_string(file).ok());
    let Some(text) = text else {
        trail.push((Rule::Saved, "none for this directory".to_string()));
        return None;
    };
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let endpoint = value
        .get("endpoint")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty());
    let provenance = value.get("provenance").and_then(Value::as_str);
    let usable = endpoint
        .filter(|url| loopback_url(url))
        .filter(|_| provenance.is_some_and(known_provenance));
    let Some(url) = usable else {
        let note = if endpoint.is_some() {
            "saved endpoint is not loopback or has no known provenance"
        } else {
            "none for this directory"
        };
        trail.push((Rule::Saved, note.to_string()));
        return None;
    };
    if provenance == Some("probe") {
        let lower = url.to_ascii_lowercase();
        let live = (lower.starts_with("http://") || lower.starts_with("https://"))
            && (lookup.probe)(url)
                .ok()
                .is_some_and(|body| connect::websocket_url_from_version(&body).is_ok());
        if !live {
            trail.push((
                Rule::Saved,
                format!(
                    "{} no longer answers",
                    cdp_client::sanitize::redact_url(url)
                ),
            ));
            return None;
        }
    }
    Some(url.to_string())
}

fn from_probe(lookup: &Lookup<'_>, trail: &mut Trail) -> Option<String> {
    let ports = lookup
        .env
        .var(cdp_client::ports::OVERRIDE)
        .and_then(cdp_client::ports::parse)
        .unwrap_or_else(cdp_client::ports::catalog);
    let mut probe = |url: &str| (lookup.probe)(url);
    if let Some(url) = cdp_client::ports::first_live(&ports, &mut probe) {
        return Some(url);
    }
    let first = ports.first().copied().unwrap_or_default();
    let last = ports.last().copied().unwrap_or_default();
    let range = if first == last {
        first.to_string()
    } else {
        format!("{first}-{last}")
    };
    trail.push((
        Rule::Probe,
        format!("nothing answered on 127.0.0.1:{range}"),
    ));
    None
}

fn http_body(url: &str) -> Result<String, String> {
    let response = cdp_client::net::http_get(url)?;
    Ok(String::from_utf8_lossy(&response.body).into_owned())
}

fn loopback_host(host: &str) -> bool {
    let name = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    name == "localhost"
        || name
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

pub fn suggest<'a>(token: &str, known: &[&'a str]) -> Option<&'a str> {
    let (name, _) = token.split_once('=').unwrap_or((token, ""));
    if name.len() < 3 {
        return None;
    }
    known
        .iter()
        .filter(|candidate| **candidate != name)
        .map(|candidate| (distance(name, candidate), *candidate))
        .filter(|(score, candidate)| *score <= if candidate.len() > 5 { 2 } else { 1 })
        .min_by_key(|(score, _)| *score)
        .map(|(_, candidate)| candidate)
}

fn distance(left: &str, right: &str) -> usize {
    let a: Vec<char> = left.chars().collect();
    let b: Vec<char> = right.chars().collect();
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in table.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in table[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (table[i - 1][j] + 1)
                .min(table[i][j - 1] + 1)
                .min(table[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(table[i - 2][j - 2] + 1);
            }
            table[i][j] = best;
        }
    }
    table[a.len()][b.len()]
}

pub const SIGN_IN: &str = "A loopback sign-in (create) opens its page in that browser when --cdp or SLICC_CDP_URL is given, or when stdin is not a terminal and a browser is found; otherwise in the system browser. jerk says on stderr where the page opened and how long it waits. For a remote CDP browser or a connected slicc cdp relay, jerk intercepts the callback in the selected browser instead of listening locally.";

pub fn help() -> String {
    let mut out = String::from("\nCDP browser (create, intercept, tab):\n");
    for text in [CONNECT_ORDER, CDP_FORMS, CDP_REPORT, SIGN_IN] {
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
