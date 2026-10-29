use crate::cli::{usage, Ctx, Io};
use crate::dispatch::{parse_flags, Flags};
use crate::exit::Exit;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use cdp_client::cdp::{self, Cdp};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) type Opener<'a> = &'a dyn Fn(Option<&str>) -> Result<Cdp, String>;

const KIND: &str = "jerk-tab";

const HELP: &str = "Usage:
  jerk tab list   [--cdp P] [--json]
  jerk tab export (--tab ID | --url PATTERN) [--cdp P] > bundle
  jerk tab import [--tab ID | --url PATTERN] [--cdp P] < bundle

export writes a tab's cookies, localStorage and sessionStorage as one base64 line.
import writes them into a tab, a new one when neither --tab nor --url is given,
and opens the exported URL there. --tab takes an id or a unique id prefix; --url
matches a substring, or the whole URL when the pattern has *. --cdp takes a port
(9222), an http(s) DevTools URL or a ws(s) URL.
";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Bundle {
    pub kind: String,
    pub v: u32,
    pub url: String,
    pub origin: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub cookies: Vec<Value>,
    #[serde(default, rename = "localStorage")]
    pub local_storage: BTreeMap<String, String>,
    #[serde(default, rename = "sessionStorage")]
    pub session_storage: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    pub id: String,
    pub url: String,
    pub title: String,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Failure {
    pub exit: Exit,
    pub message: String,
}

fn fail(exit: Exit, message: impl Into<String>) -> Failure {
    Failure {
        exit,
        message: message.into(),
    }
}

fn cdp_error(message: String) -> Failure {
    fail(Exit::Error, message.trim_end())
}

pub fn run(args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let (env, runner, exists) = (ctx.env, ctx.runner, ctx.exists);
    let open = |explicit: Option<&str>| -> Result<Cdp, String> {
        let url = locate(explicit.map(endpoint), env, runner, exists)?;
        let start = cdp_client::connect::choose_start(Some(&url), None)?;
        cdp::open(start, None)
    };
    run_with(args, ctx, io, &open)
}

#[cfg(not(target_os = "wasi"))]
fn locate(
    wanted: Option<String>,
    env: &crate::paths::Env,
    runner: &dyn crate::run::Runner,
    exists: &dyn Fn(&std::path::Path) -> bool,
) -> Result<String, String> {
    crate::browser_select::resolve_cdp_url(
        wanted.as_deref(),
        None,
        env,
        runner,
        exists,
        &mut std::io::sink(),
    )
    .map_err(|e| e.message)
}

#[cfg(target_os = "wasi")]
fn locate(
    wanted: Option<String>,
    env: &crate::paths::Env,
    _runner: &dyn crate::run::Runner,
    _exists: &dyn Fn(&std::path::Path) -> bool,
) -> Result<String, String> {
    wanted
        .or_else(|| env.var("SLICC_CDP_URL").map(str::to_string))
        .ok_or_else(|| "this kernel offers no browser: SLICC_CDP_URL is not set".to_string())
}

pub(crate) fn run_with(args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>, open: Opener) -> Exit {
    let verb = args.first().map(String::as_str);
    let rest = args.get(1..).unwrap_or(&[]);
    let bools: &[&str] = if verb == Some("list") {
        &["--json"]
    } else {
        &[]
    };
    let flags = match parse_flags(rest, &["--tab", "--url", "--cdp"], bools, 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("tab: {e}"), io),
    };
    if verb == Some("list") && (flags.value("--tab").is_some() || flags.value("--url").is_some()) {
        return usage("tab list: takes only --cdp and --json", io);
    }
    if flags.value("--tab").is_some() && flags.value("--url").is_some() {
        return usage("tab: pass --tab or --url, not both", io);
    }
    let outcome = match verb {
        Some("list") => list(&flags, open, io),
        Some("export") => export(&flags, ctx, open, io),
        Some("import") => import(&flags, ctx, open, io),
        Some("-h" | "--help") => {
            let _ = io.out.write_all(HELP.as_bytes());
            return Exit::Ok;
        }
        Some(other) => return usage(&format!("tab: unknown command {other}"), io),
        None => return usage("tab needs list, export or import", io),
    };
    match outcome {
        Ok(()) => Exit::Ok,
        Err(failure) => {
            let _ = writeln!(io.err, "jerk tab: {}", failure.message);
            failure.exit
        }
    }
}

pub(crate) fn endpoint(raw: &str) -> String {
    let raw = raw.trim();
    if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
        return format!("http://127.0.0.1:{raw}");
    }
    raw.to_string()
}

fn connect(flags: &Flags, open: Opener) -> Result<Cdp, Failure> {
    open(flags.value("--cdp")).map_err(|e| fail(Exit::Unavailable, e.trim_end()))
}

pub(crate) fn pages(cdp: &mut Cdp) -> Result<Vec<Page>, Failure> {
    let result = cdp
        .call("Target.getTargets", json!({}), None)
        .map_err(cdp_error)?;
    let infos = result
        .get("targetInfos")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(infos
        .iter()
        .filter(|info| info.get("type").and_then(Value::as_str) == Some("page"))
        .filter_map(|info| {
            let id = info.get("targetId").and_then(Value::as_str)?;
            if cdp.is_protected_target(id) {
                return None;
            }
            Some(Page {
                id: id.to_string(),
                url: text(info, "url"),
                title: text(info, "title"),
            })
        })
        .collect())
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

pub(crate) fn glob(pattern: &str, text: &str) -> bool {
    let Some((head, tail)) = pattern.split_once('*') else {
        return text.contains(pattern);
    };
    let Some(mut rest) = text.strip_prefix(head) else {
        return false;
    };
    let mut parts: Vec<&str> = tail.split('*').collect();
    let last = parts.pop().unwrap_or("");
    for part in parts {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Selector<'a> {
    Tab(&'a str),
    Url(&'a str),
}

fn selector(flags: &Flags) -> Option<Selector<'_>> {
    flags
        .value("--tab")
        .map(Selector::Tab)
        .or_else(|| flags.value("--url").map(Selector::Url))
}

pub(crate) fn pick(pages: &[Page], selector: Selector<'_>) -> Result<Page, Failure> {
    let (found, what): (Vec<&Page>, String) = match selector {
        Selector::Tab(id) => {
            let lower = id.to_ascii_lowercase();
            let exact: Vec<&Page> = pages
                .iter()
                .filter(|p| p.id.to_ascii_lowercase() == lower)
                .collect();
            let found = if exact.is_empty() {
                pages
                    .iter()
                    .filter(|p| p.id.to_ascii_lowercase().starts_with(&lower))
                    .collect()
            } else {
                exact
            };
            (found, format!("--tab {id}"))
        }
        Selector::Url(pattern) => (
            pages.iter().filter(|p| glob(pattern, &p.url)).collect(),
            format!("--url {pattern}"),
        ),
    };
    match found.as_slice() {
        [only] => Ok((*only).clone()),
        [] => Err(fail(
            Exit::Error,
            format!("no tab matches {what}; jerk tab list shows the tabs"),
        )),
        many => {
            let lines: Vec<String> = many
                .iter()
                .map(|p| format!("  {}  {}", p.id, p.url))
                .collect();
            Err(fail(
                Exit::Usage,
                format!("{} tabs match {what}:\n{}", many.len(), lines.join("\n")),
            ))
        }
    }
}

fn list(flags: &Flags, open: Opener, io: &mut Io<'_>) -> Result<(), Failure> {
    let mut cdp = connect(flags, open)?;
    let pages = pages(&mut cdp)?;
    if flags.has("--json") {
        let items: Vec<Value> = pages
            .iter()
            .map(|p| json!({ "id": p.id, "url": p.url, "title": p.title }))
            .collect();
        let _ = writeln!(io.out, "{}", Value::Array(items));
        return Ok(());
    }
    for p in &pages {
        let _ = writeln!(io.out, "{}  {}  {}", p.id, p.url, p.title);
    }
    Ok(())
}

const READ_SCRIPT: &str = "(() => { const read = (s) => { const o = {}; try { for (let i = 0; i < s.length; i++) { const k = s.key(i); if (k !== null) o[k] = s.getItem(k) ?? ''; } } catch (e) {} return o; }; let local = {}; let session = {}; try { local = read(localStorage); } catch (e) {} try { session = read(sessionStorage); } catch (e) {} return { url: location.href, origin: location.origin, title: document.title, localStorage: local, sessionStorage: session }; })()";

pub(crate) fn capture(cdp: &mut Cdp, target_id: &str) -> Result<Bundle, Failure> {
    let session = cdp::attach(cdp, target_id).map_err(cdp_error)?;
    let outcome = capture_in(cdp, &session);
    cdp::detach(cdp, &session);
    outcome
}

fn capture_in(cdp: &mut Cdp, session: &str) -> Result<Bundle, Failure> {
    let state = evaluate(cdp, session, READ_SCRIPT)?;
    let origin = text(&state, "origin");
    if !origin.starts_with("http://") && !origin.starts_with("https://") {
        return Err(fail(
            Exit::Error,
            format!(
                "the tab is on {}, which has no web origin to export",
                text(&state, "url")
            ),
        ));
    }
    let cookies = cdp
        .call("Network.getCookies", json!({}), Some(session))
        .map_err(cdp_error)?
        .get("cookies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(Bundle {
        kind: KIND.to_string(),
        v: 1,
        url: text(&state, "url"),
        origin,
        title: text(&state, "title"),
        cookies,
        local_storage: strings(&state, "localStorage"),
        session_storage: strings(&state, "sessionStorage"),
    })
}

fn strings(value: &Value, key: &str) -> BTreeMap<String, String> {
    value
        .get(key)
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn evaluate(cdp: &mut Cdp, session: &str, expression: &str) -> Result<Value, Failure> {
    let result = cdp
        .call(
            "Runtime.evaluate",
            json!({ "expression": expression, "returnByValue": true }),
            Some(session),
        )
        .map_err(cdp_error)?;
    if let Some(details) = result.get("exceptionDetails") {
        let message = details
            .pointer("/exception/description")
            .or_else(|| details.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("script failed");
        return Err(fail(Exit::Error, format!("in the tab: {message}")));
    }
    Ok(result
        .pointer("/result/value")
        .cloned()
        .unwrap_or(Value::Null))
}

fn web_origin(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https") && u.origin().ascii_serialization() == origin
    })
}

pub(crate) fn encode(bundle: &Bundle) -> String {
    let text = serde_json::to_string(bundle).unwrap_or_default();
    STANDARD.encode(text)
}

pub(crate) fn decode(text: &str) -> Result<Bundle, Failure> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err(fail(
            Exit::Usage,
            "import reads a bundle from stdin: jerk tab import < bundle",
        ));
    }
    let not_a_bundle = || fail(Exit::Usage, "stdin is not a jerk tab bundle");
    let bytes = STANDARD.decode(compact).map_err(|_| not_a_bundle())?;
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|_| not_a_bundle())?;
    if bundle.kind != KIND || !web_origin(&bundle.origin) {
        return Err(not_a_bundle());
    }
    if bundle.v != 1 {
        return Err(fail(
            Exit::Unsupported,
            format!(
                "bundle version {} is newer than this jerk understands",
                bundle.v
            ),
        ));
    }
    Ok(bundle)
}

fn export(flags: &Flags, ctx: &mut Ctx<'_>, open: Opener, io: &mut Io<'_>) -> Result<(), Failure> {
    if ctx.stdout_terminal {
        return Err(fail(
            Exit::Usage,
            "the bundle holds the tab's session cookies; redirect it: jerk tab export … > bundle",
        ));
    }
    let Some(wanted) = selector(flags) else {
        return Err(fail(
            Exit::Usage,
            "export needs --tab <id> or --url <pattern>; jerk tab list shows the tabs",
        ));
    };
    let mut cdp = connect(flags, open)?;
    let pages = pages(&mut cdp)?;
    let page = pick(&pages, wanted)?;
    let bundle = capture(&mut cdp, &page.id)?;
    let _ = writeln!(io.out, "{}", encode(&bundle));
    let _ = writeln!(
        io.err,
        "jerk tab: exported {} from {}",
        summary(&bundle),
        bundle.url
    );
    Ok(())
}

fn summary(bundle: &Bundle) -> String {
    let cookies = bundle.cookies.len();
    let storage = bundle.local_storage.len() + bundle.session_storage.len();
    format!(
        "{cookies} cookie{} and {storage} storage entr{}",
        if cookies == 1 { "" } else { "s" },
        if storage == 1 { "y" } else { "ies" }
    )
}

pub(crate) fn cookie_param(cookie: &Value) -> Option<Value> {
    let name = cookie.get("name").and_then(Value::as_str)?;
    let domain = cookie.get("domain").and_then(Value::as_str)?;
    let path = cookie.get("path").and_then(Value::as_str).unwrap_or("/");
    let secure = cookie.get("secure").and_then(Value::as_bool) == Some(true);
    let mut param = json!({
        "name": name,
        "value": cookie.get("value").and_then(Value::as_str).unwrap_or(""),
        "path": path,
        "secure": secure,
        "httpOnly": cookie.get("httpOnly").and_then(Value::as_bool) == Some(true),
    });
    if domain.starts_with('.') {
        param["domain"] = json!(domain);
    } else {
        let scheme = if secure { "https" } else { "http" };
        param["url"] = json!(format!("{scheme}://{domain}{path}"));
    }
    let session = cookie.get("session").and_then(Value::as_bool) == Some(true);
    if let Some(expires) = cookie.get("expires").and_then(Value::as_f64) {
        if !session && expires > 0.0 {
            param["expires"] = json!(expires);
        }
    }
    for key in [
        "sameSite",
        "priority",
        "sourceScheme",
        "sourcePort",
        "partitionKey",
    ] {
        if let Some(value) = cookie.get(key) {
            param[key] = value.clone();
        }
    }
    Some(param)
}

pub(crate) fn storage_script(bundle: &Bundle) -> String {
    let data = json!({
        "origin": bundle.origin,
        "localStorage": bundle.local_storage,
        "sessionStorage": bundle.session_storage,
    });
    format!(
        "(() => {{ const b = {data}; if (location.origin !== b.origin) return {{ href: location.href, written: false }}; const put = (s, v) => {{ try {{ for (const [k, x] of Object.entries(v)) s.setItem(k, x); }} catch (e) {{}} }}; put(localStorage, b.localStorage); put(sessionStorage, b.sessionStorage); return {{ href: location.href, written: true }}; }})()"
    )
}

#[derive(Debug)]
pub(crate) struct Restored {
    pub target_id: String,
    pub cookies_set: usize,
    pub landed: String,
    pub written: bool,
}

pub(crate) fn restore(
    cdp: &mut Cdp,
    target: Option<&str>,
    bundle: &Bundle,
) -> Result<Restored, Failure> {
    let target_id = match target {
        Some(id) => id.to_string(),
        None => {
            let created = cdp
                .call("Target.createTarget", json!({ "url": "about:blank" }), None)
                .map_err(cdp_error)?;
            created
                .get("targetId")
                .and_then(Value::as_str)
                .ok_or_else(|| fail(Exit::Error, "Target.createTarget returned no targetId"))?
                .to_string()
        }
    };
    let session = cdp::attach(cdp, &target_id).map_err(cdp_error)?;
    let outcome = restore_in(cdp, &session, bundle);
    cdp::detach(cdp, &session);
    let (cookies_set, landed, written) = outcome?;
    Ok(Restored {
        target_id,
        cookies_set,
        landed,
        written,
    })
}

fn restore_in(
    cdp: &mut Cdp,
    session: &str,
    bundle: &Bundle,
) -> Result<(usize, String, bool), Failure> {
    cdp.call("Page.enable", json!({}), Some(session))
        .map_err(cdp_error)?;
    let params: Vec<Value> = bundle
        .cookies
        .iter()
        .filter_map(cookie_param)
        .filter(|param| !cdp_client::shield::cookie_refused(cdp.guard(), param))
        .collect();
    let cookies_set = set_cookies(cdp, session, &params);
    if !(bundle.local_storage.is_empty() && bundle.session_storage.is_empty()) {
        navigate(cdp, session, &format!("{}/", bundle.origin))?;
        let stored = evaluate(cdp, session, &storage_script(bundle))?;
        if stored.get("written").and_then(Value::as_bool) != Some(true) {
            return Ok((cookies_set, text(&stored, "href"), false));
        }
    }
    navigate(cdp, session, &bundle.url)?;
    let landed = evaluate(cdp, session, "location.href")?
        .as_str()
        .unwrap_or("")
        .to_string();
    Ok((cookies_set, landed, true))
}

fn navigate(cdp: &mut Cdp, session: &str, url: &str) -> Result<(), Failure> {
    let from = cdp.events().len();
    cdp.call("Page.navigate", json!({ "url": url }), Some(session))
        .map_err(cdp_error)?;
    cdp.wait_load_since(session, from).map_err(cdp_error)
}

fn set_cookies(cdp: &mut Cdp, session: &str, params: &[Value]) -> usize {
    if params.is_empty() {
        return 0;
    }
    if cdp
        .call(
            "Network.setCookies",
            json!({ "cookies": params }),
            Some(session),
        )
        .is_ok()
    {
        return params.len();
    }
    params
        .iter()
        .filter(|param| {
            cdp.call("Network.setCookie", (*param).clone(), Some(session))
                .is_ok_and(|r| r.get("success").and_then(Value::as_bool) != Some(false))
        })
        .count()
}

fn import(flags: &Flags, ctx: &mut Ctx<'_>, open: Opener, io: &mut Io<'_>) -> Result<(), Failure> {
    if ctx.stdin_terminal {
        return Err(fail(
            Exit::Usage,
            "import reads a bundle from stdin: jerk tab import < bundle",
        ));
    }
    let mut text = String::new();
    ctx.input
        .read_to_string(&mut text)
        .map_err(|e| fail(Exit::Error, format!("cannot read stdin: {e}")))?;
    let bundle = decode(&text)?;
    let mut cdp = connect(flags, open)?;
    let target = match selector(flags) {
        Some(wanted) => Some(pick(&pages(&mut cdp)?, wanted)?.id),
        None => None,
    };
    let restored = restore(&mut cdp, target.as_deref(), &bundle)?;
    let skipped = bundle.cookies.len() - restored.cookies_set;
    let _ = writeln!(io.out, "{}", restored.target_id);
    let _ = writeln!(
        io.err,
        "jerk tab: imported {} into tab {}",
        summary(&bundle),
        restored.target_id
    );
    if skipped > 0 {
        let _ = writeln!(
            io.err,
            "jerk tab: {skipped} of the cookies could not be set"
        );
    }
    if !restored.written {
        return Err(fail(
            Exit::Error,
            format!(
                "the tab ended up on {}, so the storage for {} was not written",
                restored.landed, bundle.origin
            ),
        ));
    }
    let landed_origin = url::Url::parse(&restored.landed)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default();
    if landed_origin != bundle.origin {
        let _ = writeln!(io.err, "jerk tab: the tab ended up on {}", restored.landed);
    }
    Ok(())
}
