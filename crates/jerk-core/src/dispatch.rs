use crate::cli::{usage, Ctx, Io};
use crate::config::{parse_duration, parse_version, Entry, Lookup, Verb, VerbChain};
use crate::exit::Exit;
use crate::oauth;
use crate::output::{choose, Shown};
use crate::run::{Opts, Outcome, REFUSED};
use crate::template::{render_args, render_text, Vars};
use crate::{extract, fnox, mask, source, state, time, which};
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

fn step_requires<'a>(verb: &'a Verb, entry: &'a Entry) -> Option<&'a str> {
    verb.requires
        .as_deref()
        .or(entry.provider.requires.as_deref())
}

fn optional<T>(result: Result<T, Exit>) -> Result<Option<T>, Exit> {
    match result {
        Err(Exit::Refused) => Err(Exit::Refused),
        other => Ok(other.ok()),
    }
}

fn refused(entry: &Entry, code: Exit, io: &mut Io<'_>) -> Exit {
    let _ = writeln!(io.err, "jerk: {}: {REFUSED}", entry.id);
    code
}

fn program_for(
    entry: &Entry,
    command: &str,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<PathBuf, Exit> {
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

fn param_files(
    verb: &Verb,
    vars: &Vars,
    ctx: &Ctx<'_>,
) -> std::io::Result<(Vars, Vec<state::ParamFile>)> {
    let mut rendered = vars.clone();
    let mut files = Vec::new();
    for name in &verb.at_file {
        let value = vars.get(name).map(String::as_str).unwrap_or("");
        if value.is_empty() || value.starts_with('@') {
            continue;
        }
        let mut id = [0u8; 16];
        (ctx.random)(&mut id);
        let file = state::param_file(ctx.env, &fnox::hex(&id), value)?;
        rendered.insert(name.clone(), format!("@{}", file.path().display()));
        files.push(file);
    }
    Ok((rendered, files))
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
    let command = step_requires(verb, entry).unwrap_or_default();
    let run: &[String] = verb.run.as_deref().unwrap_or(&[]);
    let path = program_for(entry, command, ctx, io)?;
    check_version_for(entry, command, verb, ctx, io)?;
    let (vars, _files) = match param_files(verb, vars, ctx) {
        Ok(staged) => staged,
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: cannot write a param file: {e}", entry.id);
            return Err(Exit::Unavailable);
        }
    };
    let args = render_args(run.get(1..).unwrap_or(&[]), &vars);
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
            echo: verb.echo && interactive && ctx.stdin_terminal,
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
        Outcome::Refused => Err(refused(entry, Exit::Refused, io)),
    }
}

fn need<'a>(
    entry: &'a Entry,
    name: &str,
    chain: &'a Option<VerbChain>,
    io: &mut Io<'_>,
) -> Result<&'a VerbChain, Exit> {
    match chain {
        Some(chain) => Ok(chain),
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
    pub fields: BTreeMap<String, String>,
    pub source: &'static str,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

impl Token {
    pub fn credentials(&self, entry: &Entry) -> BTreeMap<String, String> {
        let capture_fields = entry
            .oauth_config
            .as_ref()
            .is_some_and(|config| !config.capture.fields.is_empty());
        if self.fields.is_empty() && !capture_fields {
            return entry
                .provider
                .env
                .iter()
                .map(|name| (name.clone(), self.value.clone()))
                .collect();
        }
        let mut map = BTreeMap::new();
        if let Some(primary) = entry.provider.env.first() {
            map.insert(primary.clone(), self.value.clone());
        }
        map.extend(self.fields.clone());
        map
    }

    pub fn value_for<'a>(&'a self, secret: &str, entry: &Entry) -> Option<&'a str> {
        if entry.provider.env.first().map(String::as_str) == Some(secret) {
            return Some(&self.value);
        }
        if let Some(value) = self.fields.get(secret) {
            return Some(value);
        }
        let multi = !self.fields.is_empty()
            || entry
                .provider
                .get
                .as_ref()
                .is_some_and(|chain| chain.iter().any(|verb| !verb.fields.is_empty()))
            || entry
                .oauth_config
                .as_ref()
                .is_some_and(|config| !config.capture.fields.is_empty());
        if multi {
            return None;
        }
        Some(&self.value)
    }
}

fn env_fields(entry: &Entry, ctx: &Ctx<'_>) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    if let Some(chain) = &entry.provider.get {
        for verb in chain.iter() {
            for name in verb.fields.keys() {
                if let Some(value) = ctx.env.var(name) {
                    fields.insert(name.clone(), value.to_string());
                }
            }
        }
    }
    if let Some(config) = &entry.oauth_config {
        for name in config.capture.fields.keys() {
            if let Some(value) = ctx.env.var(name) {
                if !value.is_empty() {
                    fields.insert(name.clone(), value.to_string());
                }
            }
        }
    }
    fields
}

fn grant_user(vars: &Vars) -> &str {
    if let Some(user) = vars.get("user").map(String::as_str) {
        if !user.is_empty() {
            return user;
        }
    }
    if let Some(workspace) = vars.get("workspace").map(String::as_str) {
        if !workspace.is_empty() {
            return workspace;
        }
    }
    "default"
}

fn named_fields(entry: &Entry, token: &Token) -> bool {
    !token.fields.is_empty()
        && !entry
            .provider
            .get
            .as_ref()
            .is_some_and(|chain| chain.iter().any(|verb| !verb.fields.is_empty()))
}

fn shown_secret(value: &str, terminal: bool, reveal: bool) -> String {
    match choose(false, terminal, reveal) {
        Shown::Masked => mask::mask(value),
        _ => value.to_string(),
    }
}

pub fn fetch(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<Token, Exit> {
    fetch_from(entry, vars, true, ctx, io)
}

pub fn fetch_from(
    entry: &Entry,
    vars: &Vars,
    use_env: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let named = entry
        .oauth_config
        .as_ref()
        .is_some_and(|config| !config.capture.fields.is_empty());
    let from_env = use_env
        .then(|| {
            if named {
                entry
                    .provider
                    .env
                    .first()
                    .and_then(|name| ctx.env.var(name))
                    .filter(|value| !value.is_empty())
                    .map(|value| value.to_string())
            } else {
                entry
                    .provider
                    .env
                    .iter()
                    .find_map(|n| ctx.env.var(n).map(|v| v.to_string()))
            }
        })
        .flatten();
    if let Some(value) = from_env {
        return Ok(Token {
            expires_at: None,
            scope: extract::jwt_scopes(&value),
            value,
            fields: env_fields(entry, ctx),
            source: "env",
        });
    }
    let chain = entry.provider.get.as_ref();
    let placed = chain.is_some_and(|c| c.iter().any(|v| v.oauth));
    if let Some(config) = entry.oauth_config.as_ref().filter(|_| !placed) {
        return fetch_oauth(entry, config, vars, ctx, io);
    }
    let chain = chain.expect("validated");
    run_chain(
        entry,
        "get",
        chain,
        vars,
        Exit::SignIn,
        ctx,
        io,
        |verb, io| fetch_step(entry, verb, vars, ctx, io),
    )
}

fn step_name(entry: &Entry, verb: &Verb) -> String {
    if verb.oauth {
        return "oauth".to_string();
    }
    step_requires(verb, entry).unwrap_or("read").to_string()
}

fn reason(entry: &Entry, verb: &Verb, err: &[u8]) -> String {
    let text = String::from_utf8_lossy(err);
    let line = text.lines().next().unwrap_or("printed nothing usable");
    let line = line
        .strip_prefix(&format!("jerk: {}: ", entry.id))
        .unwrap_or(line);
    if line.starts_with("sign in with") {
        return "not signed in".to_string();
    }
    let name = step_name(entry, verb);
    line.strip_prefix(&format!("{name} "))
        .unwrap_or(line)
        .to_string()
}

fn param_gate(verb: &Verb, vars: &Vars) -> Option<String> {
    for name in &verb.needs {
        if vars.get(name).map(String::as_str).unwrap_or("").is_empty() {
            return Some(format!("needs {name}"));
        }
    }
    for name in &verb.unless {
        if !vars.get(name).map(String::as_str).unwrap_or("").is_empty() {
            return Some(format!("skipped when {name} is set"));
        }
    }
    None
}

fn unavailable(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    ctx: &Ctx<'_>,
) -> Result<Option<String>, Exit> {
    if let Some(why) = param_gate(verb, vars) {
        return Ok(Some(why));
    }
    if verb.oauth || verb.run.is_none() {
        return Ok(None);
    }
    let command = step_requires(verb, entry).unwrap_or_default();
    if which::find(command, ctx.env, ctx.exists).is_none() {
        return Ok(Some("not on PATH".to_string()));
    }
    let mut err = Vec::new();
    let mut quiet = Io {
        out: &mut std::io::sink(),
        err: &mut err,
    };
    let checked = optional(check_version_for(entry, command, verb, ctx, &mut quiet))?;
    Ok(checked.is_none().then(|| reason(entry, verb, &err)))
}

fn report(entry: &Entry, name: &str, tried: &[(String, String)], io: &mut Io<'_>) {
    let _ = writeln!(io.err, "jerk: {}: no {name} step worked:", entry.id);
    for (step, why) in tried {
        let _ = writeln!(io.err, "  {step}: {why}");
    }
}

#[allow(clippy::too_many_arguments)]
fn run_chain<T>(
    entry: &Entry,
    name: &str,
    chain: &VerbChain,
    vars: &Vars,
    failed: Exit,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
    mut step: impl FnMut(&Verb, &mut Io<'_>) -> Result<T, Exit>,
) -> Result<T, Exit> {
    if let [only] = chain.0.as_slice() {
        if let Some(why) = param_gate(only, vars) {
            report(entry, name, &[(step_name(entry, only), why)], io);
            return Err(Exit::Unavailable);
        }
        return step(only, io);
    }
    let mut tried = Vec::new();
    let mut ran = false;
    for verb in chain.iter() {
        let why = unavailable(entry, verb, vars, ctx).map_err(|code| refused(entry, code, io))?;
        if let Some(why) = why {
            tried.push((step_name(entry, verb), why));
            continue;
        }
        ran = true;
        let checked = Verb {
            min_version: None,
            ..verb.clone()
        };
        let mut err = Vec::new();
        let result = step(
            &checked,
            &mut Io {
                out: &mut *io.out,
                err: &mut err,
            },
        );
        match result {
            Ok(value) => return Ok(value),
            Err(Exit::Refused) => {
                let _ = io.err.write_all(&err);
                return Err(Exit::Refused);
            }
            Err(_) => tried.push((step_name(entry, verb), reason(entry, verb, &err))),
        }
    }
    report(entry, name, &tried, io);
    if !ran {
        return Err(Exit::Unavailable);
    }
    if failed == Exit::SignIn {
        sign_in_hint(entry, io);
    }
    Err(failed)
}

pub fn pick(
    entry: &Entry,
    name: &str,
    chain: &VerbChain,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Verb, Exit> {
    if let [only] = chain.0.as_slice() {
        if let Some(why) = param_gate(only, vars) {
            report(entry, name, &[(step_name(entry, only), why)], io);
            return Err(Exit::Unavailable);
        }
        return Ok(only.clone());
    }
    let mut tried = Vec::new();
    for verb in chain.iter() {
        match unavailable(entry, verb, vars, ctx).map_err(|code| refused(entry, code, io))? {
            None => {
                return Ok(Verb {
                    min_version: None,
                    ..verb.clone()
                })
            }
            Some(why) => tried.push((step_name(entry, verb), why)),
        }
    }
    report(entry, name, &tried, io);
    Err(Exit::Unavailable)
}

fn fetch_step(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    if verb.oauth {
        let config = entry.oauth_config.as_ref().expect("validated");
        return fetch_oauth(entry, config, vars, ctx, io);
    }
    if let Some(read) = &verb.read {
        return read_token(entry, read, vars, ctx, io);
    }
    if verb.keep {
        return kept_token(entry, vars, ctx, io);
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
            step_requires(verb, entry).unwrap_or_default()
        );
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    };
    let fields = extract::fields(&ran.stdout, &verb.fields);
    let expires_at = extract::expiry(&ran.stdout, &value, verb.expiry.as_deref()).or_else(|| {
        entry
            .provider
            .max_age
            .as_deref()
            .and_then(parse_duration)
            .map(|d| ctx.now + d.as_secs())
    });
    Ok(Token {
        value,
        fields,
        source: "cli",
        expires_at,
        scope: None,
    })
}

fn kept(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>) -> Option<(String, Option<u64>)> {
    let grant = ctx
        .oauth
        .grants(entry.provider.store.as_deref())
        .get(&entry.id, grant_user(vars))?;
    let value = grant.access_token.filter(|v| !v.is_empty())?;
    grant
        .expires_at
        .is_none_or(|at| ctx.now < at)
        .then_some((value, grant.expires_at))
}

fn kept_token(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<Token, Exit> {
    let Some((value, expires_at)) = kept(entry, vars, ctx) else {
        let _ = writeln!(io.err, "jerk: {}: no kept token", entry.id);
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    };
    Ok(Token {
        value,
        fields: BTreeMap::new(),
        source: "kept",
        expires_at,
        scope: None,
    })
}

fn keep_minted(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    token: &str,
    stdout: &str,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> bool {
    let expires_at = extract::expiry(stdout, token, verb.expiry.as_deref()).or_else(|| {
        entry
            .provider
            .max_age
            .as_deref()
            .and_then(parse_duration)
            .map(|d| ctx.now + d.as_secs())
    });
    let grant = crate::grants::Grant {
        provider: entry.id.clone(),
        user: grant_user(vars).to_string(),
        refresh_token: String::new(),
        access_token: Some(token.to_string()),
        expires_at,
        scope: None,
        fields: BTreeMap::new(),
    };
    let stored = ctx
        .oauth
        .grants(entry.provider.store.as_deref())
        .store(&grant);
    if let Err(e) = &stored {
        let _ = writeln!(
            io.err,
            "jerk: {}: cannot keep the token, so save it now: {e}",
            entry.id
        );
    }
    stored.is_ok()
}

fn read_token(
    entry: &Entry,
    read: &crate::config::Read,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let path = source::path(&render_text(read.path(), vars), ctx.env);
    let value =
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| match (&read.key, &read.pointer) {
                (Some(key), _) => source::ini_value(&text, &render_text(key, vars), ctx.env),
                (None, Some(pointer)) => {
                    extract::token(&text, Some(&format!("json:{}", render_text(pointer, vars))))
                }
                (None, None) => extract::token(&text, Some("text")),
            });
    match value {
        Some(value) => Ok(Token {
            value,
            fields: BTreeMap::new(),
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

fn oauth_param_placeholder(value: &str, name: &str) -> bool {
    value.is_empty() || value == format!("{{{name}}}")
}

fn resolve_oauth_config(
    config: &jerk_oauth::config::OAuthConfig,
    vars: &Vars,
) -> Result<jerk_oauth::config::OAuthConfig, String> {
    let mut resolved = config.clone();
    if oauth_param_placeholder(&resolved.client_id, "client_id") {
        let value = vars
            .get("client_id")
            .map(String::as_str)
            .filter(|s| !s.is_empty() && !oauth_param_placeholder(s, "client_id"))
            .ok_or_else(|| "pass -p client_id=… (required for this provider)".to_string())?;
        resolved.client_id = value.to_string();
    }
    if resolved
        .client_secret
        .as_deref()
        .is_some_and(|s| oauth_param_placeholder(s, "client_secret"))
    {
        let value = vars
            .get("client_secret")
            .map(String::as_str)
            .filter(|s| !s.is_empty() && !oauth_param_placeholder(s, "client_secret"))
            .ok_or_else(|| "pass -p client_secret=… (required for this provider)".to_string())?;
        resolved.client_secret = Some(value.to_string());
    }
    if let Some(scope) = vars.get("scope").filter(|s| !s.is_empty()) {
        resolved.scope = Some(scope.clone());
    }
    Ok(resolved)
}

fn resolve_oauth(
    config: &jerk_oauth::config::OAuthConfig,
    env: &crate::paths::Env,
) -> jerk_oauth::config::OAuthConfig {
    let mut resolved = config.clone();
    resolved.client_id = source::expand(&resolved.client_id, env);
    if let Some(secret) = resolved.client_secret.take() {
        let expanded = source::expand(&secret, env);
        resolved.client_secret = (!expanded.is_empty()).then_some(expanded);
    }
    resolved
}

fn fetch_oauth(
    entry: &Entry,
    config: &jerk_oauth::config::OAuthConfig,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let user = grant_user(vars);
    let host = ctx.oauth;
    let grants = host.grants(entry.provider.store.as_deref());
    let grant = grants.get(&entry.id, user);
    let config = match resolve_oauth_config(config, vars) {
        Ok(c) => resolve_oauth(&c, ctx.env),
        Err(msg) => {
            let _ = writeln!(io.err, "jerk: {}: {msg}", entry.id);
            return Err(Exit::Usage);
        }
    };
    let refresh_from_env = config
        .refresh_env
        .as_ref()
        .and_then(|name| ctx.env.var(name))
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let machine = config.mode == jerk_oauth::config::Mode::ClientCredentials
        || config.flow == jerk_oauth::config::Flow::ClientCredentials;
    if refresh_from_env.is_none() {
        if let Some(grant) = &grant {
            if let Some(at) = &grant.access_token {
                let fresh = grant
                    .expires_at
                    .map(|exp| ctx.now + 300 < exp)
                    .unwrap_or(true);
                if fresh {
                    return Ok(Token {
                        value: at.clone(),
                        fields: grant.fields.clone(),
                        source: "oauth",
                        expires_at: grant.expires_at,
                        scope: grant.scope.clone(),
                    });
                }
                if machine {
                    return remint_client_credentials(entry, &config, vars, user, ctx, io);
                }
                sign_in_hint(entry, io);
                return Err(Exit::SignIn);
            }
        }
    }
    let from_env = refresh_from_env.is_some();
    let refresh_token = refresh_from_env.or_else(|| {
        grant
            .as_ref()
            .map(|g| g.refresh_token.clone())
            .filter(|s| !s.is_empty())
    });
    let Some(refresh_token) = refresh_token else {
        if machine {
            return remint_client_credentials(entry, &config, vars, user, ctx, io);
        }
        sign_in_hint(entry, io);
        return Err(Exit::SignIn);
    };
    match oauth::oauth_refresh(&config, &refresh_token, host, ctx.now) {
        Ok(token) => {
            if !from_env {
                if let Some(new_rt) = &token.refresh_token {
                    let updated = crate::grants::Grant {
                        provider: entry.id.clone(),
                        user: user.to_string(),
                        refresh_token: new_rt.clone(),
                        access_token: None,
                        expires_at: token.expires_at,
                        scope: token
                            .scope
                            .clone()
                            .or_else(|| grant.as_ref().and_then(|g| g.scope.clone())),
                        fields: grant
                            .as_ref()
                            .map(|item| item.fields.clone())
                            .unwrap_or_default(),
                    };
                    if let Err(e) = grants.store(&updated) {
                        let _ = writeln!(
                            io.err,
                            "jerk: {}: cannot store rotated grant: {e}",
                            entry.id
                        );
                    }
                }
            }
            Ok(Token {
                value: token.access_token,
                fields: BTreeMap::new(),
                source: if from_env { "env" } else { "oauth" },
                expires_at: token.expires_at,
                scope: token.scope.or_else(|| grant.and_then(|g| g.scope)),
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

fn remint_client_credentials(
    entry: &Entry,
    config: &jerk_oauth::config::OAuthConfig,
    vars: &Vars,
    user: &str,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Token, Exit> {
    let extra: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let token = match oauth::oauth_client_credentials(config, None, ctx.oauth, ctx.now, &extra) {
        Ok(t) => t,
        Err(e) if e.exit == Exit::SignIn || e.exit == Exit::Usage => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            sign_in_hint(entry, io);
            return Err(Exit::SignIn);
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            return Err(e.exit);
        }
    };
    let grants = ctx.oauth.grants(entry.provider.store.as_deref());
    let updated = crate::grants::Grant {
        provider: entry.id.clone(),
        user: user.to_string(),
        refresh_token: String::new(),
        access_token: Some(token.access_token.clone()),
        expires_at: token.expires_at,
        scope: token.scope.clone(),
        fields: token.fields.clone(),
    };
    if let Err(e) = grants.store(&updated) {
        let _ = writeln!(io.err, "jerk: {}: cannot store grant: {e}", entry.id);
        return Err(Exit::Error);
    }
    Ok(Token {
        value: token.access_token,
        fields: BTreeMap::new(),
        source: "oauth",
        expires_at: token.expires_at,
        scope: token.scope,
    })
}

pub fn whoami(
    entry: &Entry,
    token: &Token,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Option<String>, Exit> {
    let Some(chain) = entry.provider.user.as_ref() else {
        return Ok(None);
    };
    let scrub = token.source != "env";
    optional(run_chain(
        entry,
        "user",
        chain,
        vars,
        Exit::Error,
        ctx,
        io,
        |verb, io| {
            let ran = run_verb_env(entry, "user", verb, vars, false, scrub, ctx, io)?;
            let parse = verb.parse.as_deref().unwrap_or("text");
            let value = match parse.strip_prefix("json:") {
                Some(_) => extract::json(&ran.stdout, &crate::template::render_text(parse, vars))
                    .and_then(|v| v.as_str().map(String::from)),
                None => Some(ran.stdout.trim().to_string()),
            };
            value.filter(|v| !v.is_empty()).ok_or(Exit::Error)
        },
    ))
}

pub fn details(
    entry: &Entry,
    args: &[String],
    ctx: &Ctx<'_>,
) -> Result<Vec<(&'static str, serde_json::Value)>, Exit> {
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
    let Some(token) = optional(fetch_from(entry, &vars, false, ctx, &mut quiet))? else {
        return Ok(Vec::new());
    };
    let scopes = token
        .scope
        .clone()
        .map(|scopes| Ok(Some(scopes)))
        .unwrap_or_else(|| granted(entry, &token, &vars, ctx, &mut quiet))?;
    let user = whoami(entry, &token, &vars, ctx, &mut quiet)?;
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
    Ok(out)
}

pub fn granted(
    entry: &Entry,
    token: &Token,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Option<String>, Exit> {
    let Some(chain) = entry.provider.scopes.as_ref() else {
        return Ok(None);
    };
    let scrub = token.source != "env";
    optional(run_chain(
        entry,
        "scopes",
        chain,
        vars,
        Exit::Error,
        ctx,
        io,
        |verb, io| {
            let ran = run_verb_env(entry, "scopes", verb, vars, false, scrub, ctx, io)?;
            let scopes = match verb.parse.as_deref() {
                Some(parse) if parse.starts_with("json:") => {
                    let pointer = crate::template::render_text(parse, vars);
                    extract::json(&ran.stdout, &pointer)
                        .as_ref()
                        .and_then(extract::scopes)
                }
                _ => Some(ran.stdout.trim().to_string()).filter(|s| !s.is_empty()),
            };
            scopes.ok_or(Exit::Error)
        },
    ))
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
    let requested = requested_scope(&flags).filter(|s| !s.is_empty());
    let mut signed_in = false;
    let (token, granted_scopes) = loop {
        let token = match fetch(entry, &vars, ctx, io) {
            Ok(token) => token,
            Err(Exit::SignIn) if login && !signed_in => {
                signed_in = true;
                let code = do_create(entry, &vars, &flags, ctx, io);
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
            match granted(entry, &token, &vars, ctx, io) {
                Ok(scopes) => scopes,
                Err(code) => return code,
            }
        };
        if jerk_oauth::scopes::scopes_satisfied(scopes_now.as_deref(), requested) {
            break (token, scopes_now);
        }
        if login && !signed_in {
            signed_in = true;
            let code = do_create(entry, &vars, &flags, ctx, io);
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
    let secret = |value: &str| shown_secret(value, ctx.stdout_terminal, reveal);
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
        if named_fields(entry, &token) {
            let credentials: serde_json::Map<String, serde_json::Value> = token
                .credentials(entry)
                .into_iter()
                .map(|(name, value)| {
                    let shown = if reveal { value } else { mask::mask(&value) };
                    (name, json!(shown))
                })
                .collect();
            doc["credentials"] = serde_json::Value::Object(credentials);
        }
        let _ = writeln!(io.out, "{doc}");
        return Exit::Ok;
    }
    if named_fields(entry, &token) {
        for (name, value) in token.credentials(entry) {
            let _ = writeln!(io.out, "{name}={}", secret(&value));
        }
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
    let chain = match need(entry, "list", &entry.provider.list, io) {
        Ok(chain) => chain,
        Err(code) => return code,
    };
    let as_json = flags.has("--json");
    let listed = run_chain(
        entry,
        "list",
        chain,
        &vars,
        Exit::Error,
        ctx,
        io,
        |verb, io| list_step(entry, verb, &vars, as_json, ctx, io),
    );
    match listed {
        Ok(text) => {
            let _ = writeln!(io.out, "{text}");
            Exit::Ok
        }
        Err(code) => code,
    }
}

fn list_step(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    as_json: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<String, Exit> {
    let ran = run_verb(entry, "list", verb, vars, false, ctx, io)?;
    let value = match verb.parse.as_deref() {
        Some(parse) if parse.starts_with("json:") => {
            match extract::json(&ran.stdout, &crate::template::render_text(parse, vars)) {
                Some(value) => extract::redact(&value),
                None => {
                    let _ = writeln!(io.err, "jerk: {}: list printed no usable JSON", entry.id);
                    return Err(Exit::Error);
                }
            }
        }
        _ => json!(ran
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect::<Vec<_>>()),
    };
    if as_json {
        let items = match value {
            serde_json::Value::Array(items) => items,
            other => vec![other],
        };
        return Ok(json!({ "items": items }).to_string());
    }
    Ok(serde_json::to_string_pretty(&value).unwrap_or_default())
}

fn do_create(
    entry: &Entry,
    vars: &Vars,
    flags: &Flags,
    ctx: &mut Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    if entry.oauth_config.is_some() && entry.provider.create.is_none() {
        return oauth_create_cmd(entry, vars, flags, ctx, io);
    }
    let verb = match create_step(entry, vars, ctx, io) {
        Ok(verb) => verb,
        Err(code) => return code,
    };
    if verb.oauth {
        return oauth_create_cmd(entry, vars, flags, ctx, io);
    }
    if let Some(url) = &verb.ask {
        return match take_key(entry, &verb, url, vars, ctx, io) {
            Ok(_) => Exit::Ok,
            Err(code) => code,
        };
    }
    if verb.keep {
        return match mint_token(entry, &verb, vars, verb.interactive, ctx, io) {
            Ok(_) => Exit::Ok,
            Err(code) => code,
        };
    }
    match run_verb(entry, "create", &verb, vars, verb.interactive, ctx, io) {
        Ok(_) => Exit::Ok,
        Err(code) => code,
    }
}

fn create_step(entry: &Entry, vars: &Vars, ctx: &Ctx<'_>, io: &mut Io<'_>) -> Result<Verb, Exit> {
    let chain = need(entry, "create", &entry.provider.create, io)?;
    pick(entry, "create", chain, vars, ctx, io)
}

fn requested_scope(flags: &Flags) -> Option<&str> {
    flags
        .value("--scope")
        .or(flags.params.get("scope").map(String::as_str))
}

pub fn create(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(
        args,
        &["--scope", "--name", "--expires", "--cdp", "--seven-origin"],
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
    let mut held = Vec::new();
    let step = create_step(
        entry,
        &vars,
        ctx,
        &mut Io {
            out: &mut std::io::sink(),
            err: &mut held,
        },
    );
    if matches!(step, Err(Exit::Refused)) {
        let _ = io.err.write_all(&held);
        return Exit::Refused;
    }
    if let Some(verb) = step
        .as_ref()
        .ok()
        .filter(|v| v.parse.is_some() || v.ask.is_some())
    {
        let mut quiet = Io {
            out: &mut std::io::sink(),
            err: &mut std::io::sink(),
        };
        let signed_in = match verb.ask {
            Some(_) => match optional(fetch(entry, &vars, ctx, &mut quiet)) {
                Ok(found) => found.is_some(),
                Err(code) => return refused(entry, code, io),
            },
            None => kept(entry, &vars, ctx).is_some(),
        };
        if verb.keep && !flags.has("--force") && signed_in {
            let _ = writeln!(
                io.out,
                "{}: already signed in; use --force to sign in again",
                entry.id
            );
            return Exit::Ok;
        }
        return mint(entry, verb, &vars, &flags, ctx, io);
    }
    if !flags.has("--force") && entry.provider.get.is_some() {
        let mut quiet = Io {
            out: &mut std::io::sink(),
            err: &mut std::io::sink(),
        };
        let found = match optional(fetch(entry, &vars, ctx, &mut quiet)) {
            Ok(found) => found,
            Err(code) => return refused(entry, code, io),
        };
        if let Some(token) = found {
            let covered = match requested_scope(&flags) {
                Some(requested) => match granted(entry, &token, &vars, ctx, &mut quiet) {
                    Ok(scopes) => {
                        jerk_oauth::scopes::scopes_satisfied(scopes.as_deref(), requested)
                    }
                    Err(code) => return refused(entry, code, io),
                },
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
    let verb = match step {
        Ok(verb) => verb,
        Err(code) => {
            let _ = io.err.write_all(&held);
            return code;
        }
    };
    if verb.oauth {
        return oauth_create_cmd(entry, &vars, &flags, ctx, io);
    }
    match run_verb(entry, "create", &verb, &vars, verb.interactive, ctx, io) {
        Ok(_) => {
            let _ = writeln!(io.out, "{}: signed in", entry.id);
            Exit::Ok
        }
        Err(code) => code,
    }
}

fn oauth_create_cmd(
    entry: &Entry,
    vars: &Vars,
    flags: &Flags,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let base = entry.oauth_config.as_ref().unwrap();
    let config = match resolve_oauth_config(base, vars) {
        Ok(c) => c,
        Err(msg) => {
            let _ = writeln!(io.err, "jerk: {}: {msg}", entry.id);
            return Exit::Usage;
        }
    };
    let requested = requested_scope(flags);
    let scope = requested
        .or(vars.get("scope").map(String::as_str))
        .filter(|s| !s.is_empty())
        .or(config.scope.as_deref());
    let user = grant_user(vars);
    let host = ctx.oauth;
    let mut widened = None;

    if !flags.has("--force") {
        let grants = host.grants(entry.provider.store.as_deref());
        if let Some(g) = grants.get(&entry.id, user) {
            let mut quiet = Io {
                out: &mut std::io::sink(),
                err: &mut std::io::sink(),
            };
            let fetched = fetch_oauth(entry, base, vars, ctx, &mut quiet);
            let have = match &fetched {
                Ok(tok) => tok.scope.clone().or(g.scope.clone()),
                Err(_) => g.scope.clone(),
            };
            widened = have
                .as_deref()
                .zip(scope)
                .map(|(have, want)| jerk_oauth::scopes::union_scopes(have, want));
            if fetched.is_ok() {
                let covered = match (requested, have.as_deref()) {
                    (Some(want), have) => jerk_oauth::scopes::scopes_satisfied(have, want),
                    (None, Some(have)) => scope
                        .is_none_or(|want| jerk_oauth::scopes::scopes_satisfied(Some(have), want)),
                    (None, None) => true,
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

    #[cfg(not(target_os = "wasi"))]
    let cdp_opener = {
        let cdp_url = flags.value("--cdp");
        let seven_origin = flags
            .value("--seven-origin")
            .or(ctx.seven_origin.as_deref());
        let needs_cdp = matches!(
            config.mode,
            jerk_oauth::config::Mode::Intercept | jerk_oauth::config::Mode::Auto
        );
        if needs_cdp {
            struct Opener<'a> {
                cdp_url: Option<&'a str>,
                seven_origin: Option<&'a str>,
                env: &'a crate::paths::Env,
                runner: &'a dyn crate::run::Runner,
                exists: &'a dyn Fn(&std::path::Path) -> bool,
            }
            impl oauth::CdpOpener for Opener<'_> {
                fn open(&self) -> Result<oauth::CdpResult, oauth::OAuthError> {
                    let ws_url = crate::browser_select::resolve_cdp_url(
                        self.cdp_url,
                        self.seven_origin,
                        self.env,
                        self.runner,
                        self.exists,
                        &mut std::io::sink(),
                    )?;
                    let cdp = cdp_client::cdp::Cdp::connect(&ws_url).map_err(|message| {
                        oauth::OAuthError {
                            exit: Exit::Unavailable,
                            message,
                        }
                    })?;
                    Ok(oauth::CdpResult(cdp))
                }
            }
            let opener = Opener {
                cdp_url,
                seven_origin,
                env: ctx.env,
                runner: ctx.runner,
                exists: ctx.exists,
            };
            Some(opener)
        } else {
            None
        }
    };
    #[cfg(not(target_os = "wasi"))]
    let cdp_ref: Option<&dyn oauth::CdpOpener> =
        cdp_opener.as_ref().map(|o| o as &dyn oauth::CdpOpener);
    #[cfg(target_os = "wasi")]
    let cdp_ref: Option<&dyn oauth::CdpOpener> = None;

    let scope = widened.as_deref().or(scope);
    let config = resolve_oauth(&config, ctx.env);
    let extra: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let token = match oauth::oauth_create_with_vars(
        &config,
        scope,
        host,
        ctx.now,
        ctx.random,
        io.err,
        cdp_ref,
        &extra,
        &entry.provider.cors_origins,
    ) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            return e.exit;
        }
    };

    let recorded = token.scope.clone().or(scope.map(String::from));
    let grants = host.grants(entry.provider.store.as_deref());
    let grant = if token.refresh_token.is_some() {
        crate::grants::Grant {
            provider: entry.id.clone(),
            user: user.to_string(),
            refresh_token: token.refresh_token.clone().unwrap_or_default(),
            access_token: None,
            expires_at: token.expires_at,
            scope: recorded,
            fields: token.fields.clone(),
        }
    } else {
        crate::grants::Grant {
            provider: entry.id.clone(),
            user: user.to_string(),
            refresh_token: String::new(),
            access_token: Some(token.access_token.clone()),
            expires_at: token.expires_at,
            scope: recorded,
            fields: token.fields.clone(),
        }
    };
    if let Err(e) = grants.store(&grant) {
        let _ = writeln!(io.err, "jerk: {}: cannot store grant: {e}", entry.id);
        return Exit::Error;
    }

    let _ = writeln!(io.out, "{}: signed in", entry.id);
    Exit::Ok
}

fn check_version_for(
    entry: &Entry,
    command: &str,
    verb: &Verb,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<(), Exit> {
    let Some(min) = verb.min_version.as_deref().and_then(parse_version) else {
        return Ok(());
    };
    let probe = Verb {
        run: Some(vec![command.to_string(), "--version".to_string()]),
        requires: Some(command.to_string()),
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
                "jerk: {}: {command} {shown} is too old; this needs {a}.{b}.{c} or newer",
                entry.id
            );
            Err(Exit::Unavailable)
        }
    }
}

fn listed_id(lookup: &Lookup, stdout: &str, token: &str) -> Option<String> {
    let doc: serde_json::Value = serde_json::from_str(stdout.trim()).ok()?;
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

fn lookup_id(
    entry: &Entry,
    verb: &Verb,
    token: &str,
    vars: &Vars,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<Option<String>, Exit> {
    let Some(lookup) = verb.lookup.as_ref() else {
        return Ok(None);
    };
    let probe = Verb {
        run: Some(lookup.run.clone()),
        requires: verb.requires.clone(),
        ..Verb::default()
    };
    let ran = optional(run_verb(entry, "lookup", &probe, vars, false, ctx, io))?;
    Ok(ran.and_then(|ran| listed_id(lookup, &ran.stdout, token)))
}

pub fn mint_token(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    interactive: bool,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<(String, Option<String>), Exit> {
    let ran = run_verb(entry, "create", verb, vars, interactive, ctx, io)?;
    let Some(token) = extract::token(&ran.stdout, verb.parse.as_deref()) else {
        let _ = writeln!(io.err, "jerk: {}: create printed no token", entry.id);
        return Err(Exit::Error);
    };
    let id = verb
        .id
        .as_deref()
        .and_then(|p| extract::json(&ran.stdout, p))
        .and_then(|v| v.as_str().map(String::from));
    let id = match id {
        Some(id) => Some(id),
        None => lookup_id(entry, verb, &token, vars, ctx, io).inspect_err(|_| {
            let _ = writeln!(
                io.err,
                "jerk: {}: a token was minted, but it is not shown; revoke it by hand",
                entry.id
            );
        })?,
    };
    if verb.keep {
        let _ = keep_minted(entry, verb, vars, &token, &ran.stdout, ctx, io);
    }
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

fn take_key(
    entry: &Entry,
    verb: &Verb,
    url: &str,
    vars: &Vars,
    ctx: &mut Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<String, Exit> {
    let key = crate::paste::ask(entry, url, ctx, io)?;
    match keep_minted(entry, verb, vars, &key, "", ctx, io) {
        true => Ok(key),
        false => Err(Exit::Error),
    }
}

fn mint(
    entry: &Entry,
    verb: &Verb,
    vars: &Vars,
    flags: &Flags,
    ctx: &mut Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let minted = match &verb.ask {
        Some(url) => take_key(entry, verb, url, vars, ctx, io).map(|key| (key, None)),
        None => mint_token(entry, verb, vars, verb.interactive, ctx, io),
    };
    let (token, id) = match minted {
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
    let pasted = verb.ask.is_some();
    if pasted {
        let _ = writeln!(io.err, "jerk: {}: kept the key for get", entry.id);
    } else {
        let label = id.as_deref().unwrap_or("(no id)");
        let note = if verb.keep { ", kept for get" } else { "" };
        let _ = writeln!(io.err, "jerk: {}: created token {label}{note}", entry.id);
    }
    let shown = match choose(false, ctx.stdout_terminal && !pasted, reveal) {
        Shown::Masked => masked,
        _ => token,
    };
    let _ = writeln!(io.out, "{shown}");
    Exit::Ok
}

pub fn drop(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &["--scope"], &["--lease", "--yes"], 1) {
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
    let chain = match need(entry, "drop", &entry.provider.drop, io) {
        Ok(chain) => chain,
        Err(code) => return code,
    };
    let verb = match pick(entry, "drop", chain, &vars, ctx, io) {
        Ok(verb) => verb,
        Err(code) => return code,
    };
    if verb.oauth {
        return oauth_drop_cmd(entry, &vars, &flags, ctx, io);
    }
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
    if verb.keep {
        let grants = ctx.oauth.grants(entry.provider.store.as_deref());
        if let Err(e) = grants.remove(&entry.id, grant_user(&vars)) {
            let _ = writeln!(
                io.err,
                "jerk: {}: cannot forget the kept token: {e}",
                entry.id
            );
            return Exit::Error;
        }
        let _ = writeln!(io.out, "{}: dropped", entry.id);
        return Exit::Ok;
    }
    match run_verb(entry, "drop", &verb, &vars, verb.interactive, ctx, io) {
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
    vars: &Vars,
    flags: &Flags,
    ctx: &mut Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let user = grant_user(vars);
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
    let config = resolve_oauth(entry.oauth_config.as_ref().unwrap(), ctx.env);
    if let Some(revoke_url) = config.revoke_url.as_deref() {
        if let Some(grant) = grants.get(&entry.id, user) {
            let token = if !grant.refresh_token.is_empty() {
                grant.refresh_token.as_str()
            } else {
                grant.access_token.as_deref().unwrap_or("")
            };
            if !token.is_empty() {
                let body = format!(
                    "token={}",
                    url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>()
                );
                if let Err(e) = host.http_post(revoke_url, &body) {
                    let _ = writeln!(io.err, "jerk: {}: revoke failed: {e}", entry.id);
                }
            }
        }
    }
    if let Err(e) = grants.remove(&entry.id, user) {
        let _ = writeln!(io.err, "jerk: {}: cannot remove grant: {e}", entry.id);
        return Exit::Error;
    }
    let _ = writeln!(io.out, "{}: dropped", entry.id);
    Exit::Ok
}
