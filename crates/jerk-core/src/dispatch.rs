use crate::cli::{usage, Ctx, Io};
use crate::config::{parse_duration, parse_version, Entry, Verb};
use crate::exit::Exit;
use crate::oauth;
use crate::output::{choose, Shown};
use crate::run::{Opts, Outcome};
use crate::template::{render_args, render_text, Vars};
use crate::{extract, mask, source, state, time, which};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const LINK_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Default)]
pub struct Flags {
    pub values: BTreeMap<String, String>,
    pub bools: Vec<String>,
    pub params: BTreeMap<String, String>,
    pub positionals: Vec<String>,
}

impl Flags {
    pub fn has(&self, name: &str) -> bool {
        self.bools.iter().any(|b| b == name)
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

pub fn parse_flags(
    args: &[String],
    values: &[&str],
    bools: &[&str],
    positionals: usize,
) -> Result<Flags, String> {
    let mut flags = Flags::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        if name == "-p" || name == "--param" {
            let pair = match inline {
                Some(v) => v,
                None => iter.next().cloned().ok_or("-p needs key=value")?,
            };
            let (k, v) = pair
                .split_once('=')
                .ok_or(format!("-p {pair}: needs key=value"))?;
            flags.params.insert(k.to_string(), v.to_string());
        } else if values.contains(&name) {
            let value = match inline {
                Some(v) => v,
                None => iter.next().cloned().unwrap_or_default(),
            };
            if value.is_empty() || value.starts_with('-') {
                return Err(format!("{name} needs a value"));
            }
            flags.values.insert(name.to_string(), value);
        } else if bools.contains(&name) && inline.is_none() {
            flags.bools.push(name.to_string());
        } else if arg.starts_with('-') {
            return Err(format!("unknown option {arg}"));
        } else if flags.positionals.len() < positionals {
            flags.positionals.push(arg.clone());
        } else {
            return Err(format!("unexpected {arg}"));
        }
    }
    Ok(flags)
}

pub fn vars(entry: &Entry, flags: &Flags) -> Result<Vars, String> {
    let mut vars: Vars = entry.provider.params.clone();
    for (k, v) in &flags.params {
        if !entry.provider.params.contains_key(k) {
            return Err(format!("{} has no param {k}", entry.id));
        }
        vars.insert(k.clone(), v.clone());
    }
    for (flag, var) in [
        ("--scope", "scope"),
        ("--name", "name"),
        ("--expires", "expires"),
    ] {
        if let Some(value) = flags.value(flag) {
            vars.insert(var.to_string(), value.to_string());
        }
    }
    if let Some(id) = flags.positionals.first() {
        vars.insert("id".to_string(), id.clone());
    }
    Ok(vars)
}

pub struct Ran {
    pub stdout: String,
}

fn program(entry: &Entry, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<PathBuf, Exit> {
    let command = entry.provider.requires.as_deref().unwrap_or_default();
    match which::find(command, ctx.env, ctx.exists) {
        Some(path) => Ok(path),
        None => {
            let _ = writeln!(
                io.err,
                "jerk: {}: {command} is not on PATH; install it first",
                entry.id
            );
            Err(Exit::Unavailable)
        }
    }
}

pub fn run_verb(
    entry: &Entry,
    name: &str,
    verb: &Verb,
    vars: &Vars,
    interactive: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Ran, Exit> {
    run_verb_env(entry, name, verb, vars, interactive, true, ctx, io)
}

#[allow(clippy::too_many_arguments)]
pub fn run_verb_env(
    entry: &Entry,
    name: &str,
    verb: &Verb,
    vars: &Vars,
    interactive: bool,
    scrub: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Ran, Exit> {
    let run: &[String] = verb.run.as_deref().unwrap_or(&[]);
    let path = program(entry, ctx, io)?;
    let args = render_args(run.get(1..).unwrap_or(&[]), vars);
    let linked = interactive && ctx.via_link;
    let interactive = interactive && !ctx.via_link;
    let fallback = if linked {
        LINK_TIMEOUT
    } else {
        DEFAULT_TIMEOUT
    };
    let timeout = verb
        .timeout
        .as_deref()
        .and_then(parse_duration)
        .or((!interactive).then_some(fallback));
    let outcome = {
        let progress = std::cell::RefCell::new(&mut *io.err);
        let forward = |line: &str| {
            let shown = crate::mask::mask_words(line);
            if !shown.trim().is_empty() {
                let _ = writeln!(progress.borrow_mut(), "{shown}");
            }
        };
        let opts = Opts {
            interactive,
            capture: verb.parse.is_some(),
            timeout,
            remove_env: if scrub { &entry.provider.env } else { &[] },
            input: None,
            on_stderr: linked.then_some(&forward as &dyn Fn(&str)),
        };
        ctx.runner.run(&path, &args, &opts)
    };
    let cli = run.first().map(String::as_str).unwrap_or_default();
    match outcome {
        Outcome::Exited {
            code: 0, stdout, ..
        } => Ok(Ran { stdout }),
        Outcome::Exited { code, .. } => {
            let _ = writeln!(io.err, "jerk: {}: {cli} {name} exited {code}", entry.id);
            Err(Exit::Error)
        }
        Outcome::TimedOut => {
            let _ = writeln!(io.err, "jerk: {}: {cli} did not answer in time", entry.id);
            Err(Exit::SignIn)
        }
        Outcome::Failed(e) => {
            let _ = writeln!(io.err, "jerk: {}: cannot run {cli}: {e}", entry.id);
            Err(Exit::Unavailable)
        }
    }
}

fn need<'a>(
    entry: &'a Entry,
    name: &str,
    verb: &'a Option<Verb>,
    io: &mut Io<'_>,
) -> Result<&'a Verb, Exit> {
    match verb {
        Some(verb) => Ok(verb),
        None => {
            let _ = writeln!(io.err, "jerk: {} has no {name} command", entry.id);
            Err(Exit::Unsupported)
        }
    }
}

fn sign_in_hint(entry: &Entry, io: &mut Io<'_>) {
    let _ = writeln!(
        io.err,
        "jerk: {}: sign in with jerk {} create",
        entry.id, entry.id
    );
}

pub struct Token {
    pub value: String,
    pub source: &'static str,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

pub fn fetch(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<Token, Exit> {
    if let Some(value) = entry.provider.env.first().and_then(|n| ctx.env.var(n)) {
        return Ok(Token {
            expires_at: None,
            value: value.to_string(),
            source: "env",
            scope: None,
        });
    }
    if let Some(oauth_config) = &entry.oauth_config {
        return fetch_oauth(entry, oauth_config, vars, ctx, io);
    }
    let verb = entry.provider.get.as_ref().expect("validated");
    if let Some(read) = &verb.read {
        return read_token(entry, read, vars, ctx, io);
    }
    let ran = match run_verb(entry, "get", verb, vars, false, ctx, io) {
        Ok(ran) => ran,
        Err(Exit::Error) => {
            sign_in_hint(entry, io);
            return Err(Exit::SignIn);
        }
        Err(code) => return Err(code),
    };
    let Some(value) = extract::token(&ran.stdout, verb.parse.as_deref()) else {
        let _ = writeln!(
            io.err,
            "jerk: {}: {} printed no token",
            entry.id,
            entry.provider.requires.as_deref().unwrap_or_default()
        );
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    };
    let expires_at = if verb.expiry.as_deref() == Some("jwt") {
        extract::jwt_expiry(&value)
    } else {
        None
    }
    .or_else(|| {
        entry
            .provider
            .max_age
            .as_deref()
            .and_then(parse_duration)
            .map(|d| ctx.now + d.as_secs())
    });
    Ok(Token {
        value,
        source: "cli",
        expires_at,
        scope: None,
    })
}

fn read_token(
    entry: &Entry,
    read: &crate::config::Read,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let path = source::path(&render_text(&read.ini, vars), ctx.env);
    let key = render_text(&read.key, vars);
    let value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| source::ini_value(&text, &key, ctx.env));
    match value {
        Some(value) => Ok(Token {
            value,
            source: "file",
            expires_at: None,
            scope: None,
        }),
        None => {
            let _ = writeln!(io.err, "jerk: {}: no token in {}", entry.id, path.display());
            sign_in_hint(entry, io);
            Err(Exit::SignIn)
        }
    }
}

fn fetch_oauth(
    entry: &Entry,
    config: &jerk_oauth::config::OAuthConfig,
    _vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let user = entry
        .provider
        .params
        .get("user")
        .map(String::as_str)
        .unwrap_or("default");
    let host = ctx.oauth;
    let grants = host.grants(entry.provider.store.as_deref());
    let grant = grants.get(&entry.id, user);
    let Some(grant) = grant else {
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    };
    if let Some(at) = &grant.access_token {
        let fresh = grant
            .expires_at
            .map(|exp| ctx.now + 300 < exp)
            .unwrap_or(true);
        if fresh {
            return Ok(Token {
                value: at.clone(),
                source: "oauth",
                expires_at: grant.expires_at,
                scope: grant.scope.clone(),
            });
        }
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    }
    if grant.refresh_token.is_empty() {
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    }
    match oauth::oauth_refresh(config, &grant.refresh_token, host, ctx.now) {
        Ok(token) => {
            if let Some(new_rt) = &token.refresh_token {
                let updated = crate::grants::Grant {
                    provider: entry.id.clone(),
                    user: user.to_string(),
                    refresh_token: new_rt.clone(),
                    access_token: None,
                    expires_at: token.expires_at,
                    scope: token.scope.clone(),
                };
                if let Err(e) = grants.store(&updated) {
                    let _ = writeln!(
                        io.err,
                        "jerk: {}: cannot store rotated grant: {e}",
                        entry.id
                    );
                }
            }
            Ok(Token {
                value: token.access_token,
                source: "oauth",
                expires_at: token.expires_at,
                scope: token.scope.or(grant.scope),
            })
        }
        Err(e) if e.exit == Exit::SignIn => {
            sign_in_hint(entry, io);
            Err(Exit::SignIn)
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: refresh failed: {e}", entry.id);
            Err(e.exit)
        }
    }
}

pub fn whoami(
    entry: &Entry,
    token: &Token,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Option<String> {
    let verb = entry.provider.user.as_ref()?;
    let ran = run_verb_env(
        entry,
        "user",
        verb,
        vars,
        false,
        token.source != "env",
        ctx,
        io,
    )
    .ok()?;
    let parse = verb.parse.as_deref().unwrap_or("text");
    let value = match parse.strip_prefix("json:") {
        Some(_) => extract::json(&ran.stdout, &crate::template::render_text(parse, vars))?
            .as_str()?
            .to_string(),
        None => ran.stdout.trim().to_string(),
    };
    (!value.is_empty()).then_some(value)
}

pub fn details(
    entry: &Entry,
    args: &[String],
    ctx: &Ctx<'_>,
) -> Vec<(&'static str, serde_json::Value)> {
    let mut quiet = Io {
        out: &mut std::io::sink(),
        err: &mut std::io::sink(),
    };
    let flags = parse_flags(
        args,
        &["--scope", "--name", "--expires"],
        &["--force", "--reveal", "--json"],
        0,
    )
    .unwrap_or_default();
    let vars = vars(entry, &flags).unwrap_or_default();
    let Ok(token) = fetch(entry, &vars, ctx, &mut quiet) else {
        return Vec::new();
    };
    let scopes = token
        .scope
        .clone()
        .or_else(|| granted(entry, &token, &vars, ctx, &mut quiet));
    let user = whoami(entry, &token, &vars, ctx, &mut quiet);
    let mut out = Vec::new();
    if let Some(user) = user {
        out.push(("user", json!(user)));
    }
    if let Some(scopes) = scopes {
        out.push(("scopes", json!(scopes)));
    }
    if let Some(at) = token.expires_at {
        out.push(("expires_at", json!(time::rfc3339(at))));
    }
    out
}

pub fn granted(
    entry: &Entry,
    token: &Token,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Option<String> {
    let verb = entry.provider.scopes.as_ref()?;
    let scrub = token.source != "env";
    let ran = run_verb_env(entry, "scopes", verb, vars, false, scrub, ctx, io).ok()?;
    match verb.parse.as_deref() {
        Some(parse) if parse.starts_with("json:") => {
            let pointer = crate::template::render_text(parse, vars);
            extract::json(&ran.stdout, &pointer)
                .as_ref()
                .and_then(extract::scopes)
        }
        _ => Some(ran.stdout.trim().to_string()).filter(|s| !s.is_empty()),
    }
}

pub fn get(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(
        args,
        &["--scope"],
        &["--reveal", "--refresh", "--login", "--json"],
        0,
    ) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} get: {e}", entry.id), io),
    };
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    let login = flags.has("--login") && ctx.stdin_terminal && ctx.stdout_terminal;
    let requested = flags.value("--scope");
    let mut signed_in = false;
    let (token, granted_scopes) = loop {
        let token = match fetch(entry, &vars, ctx, io) {
            Ok(token) => token,
            Err(Exit::SignIn) if login && !signed_in => {
                signed_in = true;
                let code = do_create(entry, &vars, ctx, io);
                if code != Exit::Ok {
                    return code;
                }
                continue;
            }
            Err(code) => return code,
        };
        let Some(requested) = requested else {
            break (token, None);
        };
        let scopes_now = if token.scope.is_some() {
            token.scope.clone()
        } else {
            granted(entry, &token, &vars, ctx, io)
        };
        if jerk_oauth::scopes::scopes_satisfied(scopes_now.as_deref(), requested) {
            break (token, scopes_now);
        }
        if login && !signed_in {
            signed_in = true;
            let code = do_create(entry, &vars, ctx, io);
            if code != Exit::Ok {
                return code;
            }
            continue;
        }
        let _ = writeln!(
            io.err,
            "jerk: {}: the token does not cover {requested}; run jerk {} create --scope {requested}",
            entry.id, entry.id
        );
        return Exit::SignIn;
    };
    let expires = token.expires_at.map(time::rfc3339);
    let masked = mask::mask(&token.value);
    if flags.has("--refresh") {
        let until = expires
            .as_deref()
            .map(|e| format!(", expires {e}"))
            .unwrap_or_default();
        let _ = writeln!(io.out, "{}: token ok ({}{until})", entry.id, token.source);
        return Exit::Ok;
    }
    let reveal = flags.has("--reveal");
    if flags.has("--json") {
        let mut doc = json!({
            "provider": entry.id,
            "source": token.source,
            "masked": masked,
            "expires_at": expires,
            "scopes": granted_scopes,
        });
        if reveal {
            doc["token"] = json!(token.value);
        }
        let _ = writeln!(io.out, "{doc}");
        return Exit::Ok;
    }
    let shown = match choose(false, ctx.stdout_terminal, reveal) {
        Shown::Masked => masked,
        _ => token.value,
    };
    let _ = writeln!(io.out, "{shown}");
    Exit::Ok
}

pub fn list(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &[], &["--json"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} list: {e}", entry.id), io),
    };
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    let verb = match need(entry, "list", &entry.provider.list, io) {
        Ok(verb) => verb,
        Err(code) => return code,
    };
    let ran = match run_verb(entry, "list", verb, &vars, false, ctx, io) {
        Ok(ran) => ran,
        Err(code) => return code,
    };
    let value = match verb.parse.as_deref() {
        Some(parse) if parse.starts_with("json:") => {
            match extract::json(&ran.stdout, &crate::template::render_text(parse, &vars)) {
                Some(value) => extract::redact(&value),
                None => {
                    let _ = writeln!(io.err, "jerk: {}: list printed no usable JSON", entry.id);
                    return Exit::Error;
                }
            }
        }
        _ => json!(ran
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()),
    };
    let text = if flags.has("--json") {
        let items = match value {
            serde_json::Value::Array(items) => items,
            other => vec![other],
        };
        json!({ "items": items }).to_string()
    } else {
        serde_json::to_string_pretty(&value).unwrap_or_default()
    };
    let _ = writeln!(io.out, "{text}");
    Exit::Ok
}

fn do_create(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Exit {
    if entry.oauth_config.is_some() && entry.provider.create.is_none() {
        let flags = Flags::default();
        return oauth_create_cmd(entry, vars, &flags, ctx, io);
    }
    create_run(entry, vars, ctx, io)
}

fn create_run(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let verb = match need(entry, "create", &entry.provider.create, io) {
        Ok(verb) => verb,
        Err(code) => return code,
    };
    match run_verb(entry, "create", verb, vars, verb.interactive, ctx, io) {
        Ok(_) => Exit::Ok,
        Err(code) => code,
    }
}

pub fn create(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(
        args,
        &["--scope", "--name", "--expires"],
        &["--force", "--reveal", "--json"],
        0,
    ) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} create: {e}", entry.id), io),
    };
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    if entry.provider.create.is_none() && entry.oauth_config.is_some() {
        return oauth_create_cmd(entry, &vars, &flags, ctx, io);
    }
    if let Some(verb) = entry.provider.create.as_ref().filter(|v| v.parse.is_some()) {
        return mint(entry, verb, &vars, &flags, ctx, io);
    }
    if !flags.has("--force") && entry.provider.get.is_some() {
        let mut quiet = Io {
            out: &mut std::io::sink(),
            err: &mut std::io::sink(),
        };
        if let Ok(token) = fetch(entry, &vars, ctx, &mut quiet) {
            let covered = match flags.value("--scope") {
                Some(requested) => jerk_oauth::scopes::scopes_satisfied(
                    granted(entry, &token, &vars, ctx, &mut quiet).as_deref(),
                    requested,
                ),
                None => true,
            };
            if covered {
                let _ = writeln!(
                    io.out,
                    "{}: already signed in; use --force to sign in again",
                    entry.id
                );
                return Exit::Ok;
            }
        }
    }
    let code = create_run(entry, &vars, ctx, io);
    if code == Exit::Ok {
        let _ = writeln!(io.out, "{}: signed in", entry.id);
    }
    code
}

fn oauth_create_cmd(
    entry: &Entry,
    vars: &Vars,
    flags: &Flags,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let config = entry.oauth_config.as_ref().unwrap();
    let scope = flags
        .value("--scope")
        .or(vars.get("scope").map(String::as_str));
    let user = vars.get("user").map(String::as_str).unwrap_or("default");
    let host = ctx.oauth;

    if !flags.has("--force") {
        let grants = host.grants(entry.provider.store.as_deref());
        if let Some(g) = grants.get(&entry.id, user) {
            let mut quiet = Io {
                out: &mut std::io::sink(),
                err: &mut std::io::sink(),
            };
            if let Ok(tok) = fetch_oauth(entry, config, vars, ctx, &mut quiet) {
                let covered = match scope {
                    Some(requested) => {
                        let have = tok.scope.or(g.scope);
                        jerk_oauth::scopes::scopes_satisfied(have.as_deref(), requested)
                    }
                    None => true,
                };
                if covered {
                    let _ = writeln!(
                        io.out,
                        "{}: already signed in; use --force to sign in again",
                        entry.id
                    );
                    return Exit::Ok;
                }
            }
        }
    }

    let token = match oauth::oauth_create(config, scope, host, ctx.now, ctx.random, io.err) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            return e.exit;
        }
    };

    let grants = host.grants(entry.provider.store.as_deref());
    let grant = if token.refresh_token.is_some() {
        crate::grants::Grant {
            provider: entry.id.clone(),
            user: user.to_string(),
            refresh_token: token.refresh_token.clone().unwrap_or_default(),
            access_token: None,
            expires_at: token.expires_at,
            scope: token.scope.clone(),
        }
    } else {
        crate::grants::Grant {
            provider: entry.id.clone(),
            user: user.to_string(),
            refresh_token: String::new(),
            access_token: Some(token.access_token.clone()),
            expires_at: token.expires_at,
            scope: token.scope.clone(),
        }
    };
    if let Err(e) = grants.store(&grant) {
        let _ = writeln!(io.err, "jerk: {}: cannot store grant: {e}", entry.id);
        return Exit::Error;
    }

    let _ = writeln!(io.out, "{}: signed in", entry.id);
    Exit::Ok
}

fn check_version(entry: &Entry, verb: &Verb, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<(), Exit> {
    let Some(min) = verb.min_version.as_deref().and_then(parse_version) else {
        return Ok(());
    };
    let probe = Verb {
        run: Some(vec![
            entry.provider.requires.clone().unwrap_or_default(),
            "--version".to_string(),
        ]),
        ..Verb::default()
    };
    let ran = run_verb(entry, "version", &probe, &Vars::new(), false, ctx, io)?;
    match parse_version(&ran.stdout) {
        Some(found) if found >= min => Ok(()),
        found => {
            let shown = found
                .map(|(a, b, c)| format!("{a}.{b}.{c}"))
                .unwrap_or_else(|| "unknown".to_string());
            let (a, b, c) = min;
            let _ = writeln!(
                io.err,
                "jerk: {}: {} {shown} is too old; this needs {a}.{b}.{c} or newer",
                entry.id,
                entry.provider.requires.as_deref().unwrap_or_default()
            );
            Err(Exit::Unavailable)
        }
    }
}

fn lookup_id(
    entry: &Entry,
    verb: &Verb,
    token: &str,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Option<String> {
    let lookup = verb.lookup.as_ref()?;
    let probe = Verb {
        run: Some(lookup.run.clone()),
        ..Verb::default()
    };
    let ran = run_verb(entry, "lookup", &probe, vars, false, ctx, io).ok()?;
    let doc: serde_json::Value = serde_json::from_str(ran.stdout.trim()).ok()?;
    let items = doc.pointer(&lookup.items)?.as_array()?;
    items
        .iter()
        .filter(|item| {
            item.pointer(&lookup.prefix)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|p| !p.is_empty() && token.starts_with(p))
        })
        .find_map(|item| item.pointer(&lookup.id)?.as_str().map(String::from))
}

pub fn mint_token(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    interactive: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<(String, Option<String>), Exit> {
    check_version(entry, verb, ctx, io)?;
    let ran = run_verb(entry, "create", verb, vars, interactive, ctx, io)?;
    let Some(token) = extract::token(&ran.stdout, verb.parse.as_deref()) else {
        let _ = writeln!(io.err, "jerk: {}: create printed no token", entry.id);
        return Err(Exit::Error);
    };
    let id = verb
        .id
        .as_deref()
        .and_then(|p| extract::json(&ran.stdout, p))
        .and_then(|v| v.as_str().map(String::from))
        .or_else(|| lookup_id(entry, verb, &token, vars, ctx, io));
    if let Some(id) = &id {
        if let Err(e) = state::record(ctx.env, &entry.id, id) {
            let _ = writeln!(
                io.err,
                "jerk: {}: cannot record the token id: {e}",
                entry.id
            );
        }
    }
    Ok((token, id))
}

fn mint(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    flags: &Flags,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let (token, id) = match mint_token(entry, verb, vars, verb.interactive, ctx, io) {
        Ok(minted) => minted,
        Err(code) => return code,
    };
    let masked = mask::mask(&token);
    let reveal = flags.has("--reveal");
    if flags.has("--json") {
        let mut doc = json!({ "provider": entry.id, "id": id, "masked": masked });
        if reveal {
            doc["token"] = json!(token);
        }
        let _ = writeln!(io.out, "{doc}");
        return Exit::Ok;
    }
    let label = id.as_deref().unwrap_or("(no id)");
    let _ = writeln!(io.err, "jerk: {}: created token {label}", entry.id);
    let shown = match choose(false, ctx.stdout_terminal, reveal) {
        Shown::Masked => masked,
        _ => token,
    };
    let _ = writeln!(io.out, "{shown}");
    Exit::Ok
}

pub fn drop(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &[], &["--lease", "--yes"], 1) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} drop: {e}", entry.id), io),
    };
    if flags.has("--lease") {
        return crate::fnox::drop_lease(entry, &flags, ctx, io);
    }
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    if entry.oauth_config.is_some() && entry.provider.drop.is_none() {
        return oauth_drop_cmd(entry, &vars, &flags, ctx, io);
    }
    let verb = match need(entry, "drop", &entry.provider.drop, io) {
        Ok(verb) => verb,
        Err(code) => return code,
    };
    if !flags.has("--yes") {
        if !ctx.stdin_terminal {
            return usage(
                &format!("{} drop: pass --yes when not on a terminal", entry.id),
                io,
            );
        }
        let what = flags
            .positionals
            .first()
            .map(|id| format!(" {id}"))
            .unwrap_or_default();
        let _ = write!(io.err, "Drop {}{what}? [y/N] ", entry.id);
        let _ = io.err.flush();
        let mut answer = String::new();
        let _ = ctx.input.read_line(&mut answer);
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            let _ = writeln!(io.err, "jerk: cancelled");
            return Exit::Error;
        }
    }
    match run_verb(entry, "drop", verb, &vars, verb.interactive, ctx, io) {
        Ok(_) => {
            if let Some(id) = flags.positionals.first() {
                let _ = state::forget(ctx.env, &entry.id, id);
            }
            let _ = writeln!(io.out, "{}: dropped", entry.id);
            Exit::Ok
        }
        Err(code) => code,
    }
}

fn oauth_drop_cmd(
    entry: &Entry,
    _vars: &Vars,
    flags: &Flags,
    ctx: &mut Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let user = entry
        .provider
        .params
        .get("user")
        .map(String::as_str)
        .unwrap_or("default");
    if !flags.has("--yes") {
        if !ctx.stdin_terminal {
            return usage(
                &format!("{} drop: pass --yes when not on a terminal", entry.id),
                io,
            );
        }
        let _ = write!(io.err, "Drop {}? [y/N] ", entry.id);
        let _ = io.err.flush();
        let mut answer = String::new();
        let _ = ctx.input.read_line(&mut answer);
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            let _ = writeln!(io.err, "jerk: cancelled");
            return Exit::Error;
        }
    }
    let host = ctx.oauth;
    let grants = host.grants(entry.provider.store.as_deref());
    if let Err(e) = grants.remove(&entry.id, user) {
        let _ = writeln!(io.err, "jerk: {}: cannot remove grant: {e}", entry.id);
        return Exit::Error;
    }
    let _ = writeln!(io.out, "{}: dropped", entry.id);
    Exit::Ok
}
