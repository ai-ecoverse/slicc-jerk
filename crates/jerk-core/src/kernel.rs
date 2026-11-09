use crate::cli::{Ctx, Io};
use crate::config::{Config, Entry};
use crate::exit::Exit;
use crate::sealed::{PollResult, Sealed, SealedError};
use serde_json::{json, Value};

pub mod listing;

pub fn list(
    config: &Config,
    args: &[String],
    ctx: &Ctx<'_>,
    sealed: &dyn Sealed,
    io: &mut Io<'_>,
) -> Exit {
    let json = match args.first().map(String::as_str) {
        Some("--json") if args.len() == 1 => true,
        None => false,
        Some(other) => {
            let _ = writeln!(
                io.err,
                "jerk: list: unexpected {other}\nRun jerk --help for usage."
            );
            return Exit::Usage;
        }
    };

    let kernel_providers: Vec<KernelRow> = config
        .providers
        .values()
        .map(|entry| KernelRow {
            provider: entry.id.clone(),
            name: entry
                .provider
                .name
                .clone()
                .unwrap_or_else(|| entry.id.clone()),
            source: "seven".to_string(),
            cli: entry
                .provider
                .requires
                .as_deref()
                .map(|command| {
                    if crate::which::find(command, ctx.env, ctx.exists).is_some() {
                        command.to_string()
                    } else {
                        "missing".to_string()
                    }
                })
                .unwrap_or_else(|| "-".to_string()),
            env: entry.provider.env.clone(),
        })
        .collect();

    let host_providers = host_list(sealed).unwrap_or_default();

    let mut all = kernel_providers;
    all.extend(host_providers);

    if json {
        let items: Vec<Value> = all
            .iter()
            .map(|row| {
                json!({
                    "provider": row.provider,
                    "name": row.name,
                    "source": row.source,
                    "cli": row.cli,
                    "env": row.env,
                })
            })
            .collect();
        let _ = writeln!(io.out, "{}", Value::Array(items));
        return Exit::Ok;
    }

    let rows: Vec<[String; 4]> = all
        .iter()
        .map(|row| {
            [
                row.provider.clone(),
                row.name.clone(),
                row.source.clone(),
                row.cli.clone(),
            ]
        })
        .collect();
    let _ = io
        .out
        .write_all(crate::cli::table(&["PROVIDER", "NAME", "SOURCE", "CLI"], &rows).as_bytes());
    Exit::Ok
}

pub struct KernelRow {
    pub provider: String,
    pub name: String,
    pub source: String,
    pub cli: String,
    pub env: Vec<String>,
}

pub fn host_list(sealed: &dyn Sealed) -> Result<Vec<KernelRow>, SealedError> {
    let body = json!({ "v": 1, "op": "request", "kind": "jerk", "jop": "list" });
    let reply = sealed.request(&body)?;
    if let Some(exit) = reply.get("exit").and_then(Value::as_i64) {
        if exit != 0 {
            return Ok(Vec::new());
        }
    }
    let items = reply
        .get("rows")
        .or_else(|| reply.get("items"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .filter_map(|item| {
            let provider = item.get("provider")?.as_str()?.to_string();
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&provider)
                .to_string();
            let device = item.get("device").and_then(Value::as_str).unwrap_or("cli");
            let env = item
                .get("env")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            Some(KernelRow {
                provider,
                name,
                source: format!("host:{device}"),
                cli: "host".to_string(),
                env,
            })
        })
        .collect())
}

pub fn get(
    entry: &Entry,
    args: &[String],
    env_val: Option<&str>,
    sealed: &dyn Sealed,
    io: &mut Io<'_>,
) -> Exit {
    let json = args.iter().any(|a| a == "--json");
    for arg in args {
        if arg.starts_with('-') && arg != "--json" && arg != "--reveal" {
            let _ = writeln!(
                io.err,
                "jerk: {} get: unexpected {arg}\nRun jerk --help for usage.",
                entry.id
            );
            return Exit::Usage;
        }
    }

    if let Some(value) = env_val {
        let name = entry
            .provider
            .env
            .first()
            .map(String::as_str)
            .unwrap_or("JERK_TOKEN");
        let placeholder = match crate::sealed::seal_value(sealed, name, value) {
            Ok(value) => value,
            Err(e) => {
                let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
                return Exit::Error;
            }
        };
        if json {
            let doc = json!({
                "provider": entry.id,
                "source": "env",
                "placeholder": placeholder,
            });
            let _ = writeln!(io.out, "{doc}");
        } else {
            let _ = writeln!(io.out, "{placeholder}");
        }
        return Exit::Ok;
    }

    let body = json!({
        "v": 1,
        "op": "request",
        "kind": "jerk",
        "jop": "status",
        "provider": entry.id,
    });
    match sealed.request(&body) {
        Ok(reply) => {
            if let Some(exit) = reply.get("exit").and_then(Value::as_i64) {
                if exit != 0 {
                    let msg = reply
                        .get("message")
                        .and_then(Value::as_str)
                        .filter(|_| !(is_mcp(&entry.id) && exit == 3))
                        .unwrap_or("not signed in");
                    let _ = writeln!(io.err, "jerk: {}: {msg}", entry.id);
                    hint_create(&entry.id, io);
                    return exit_from(exit);
                }
            }
            if let Some(placeholder) = reply
                .get("placeholder")
                .and_then(Value::as_str)
                .filter(|v| crate::sealed::is_placeholder(v))
            {
                if json {
                    let doc = json!({
                        "provider": entry.id,
                        "source": "sealed",
                        "placeholder": placeholder,
                    });
                    let _ = writeln!(io.out, "{doc}");
                } else {
                    let _ = writeln!(io.out, "{placeholder}");
                }
                return Exit::Ok;
            }
            let _ = writeln!(io.err, "jerk: {}: no sealed record", entry.id);
            hint_create(&entry.id, io);
            Exit::SignIn
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            hint_create(&entry.id, io);
            Exit::SignIn
        }
    }
}

pub fn local_get(
    entry: &Entry,
    args: &[String],
    ctx: &Ctx<'_>,
    sealed: &dyn Sealed,
    io: &mut Io<'_>,
) -> Exit {
    let flags = match crate::dispatch::parse_flags(args, &[], &["--json", "--reveal"], 0) {
        Ok(flags) => flags,
        Err(e) => return crate::cli::usage(&format!("{} get: {e}", entry.id), io),
    };
    let vars = match crate::dispatch::vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return crate::cli::usage(&e, io),
    };
    let show = |value: &str, source: &str, io: &mut Io<'_>| {
        if flags.has("--json") {
            let _ = writeln!(
                io.out,
                "{}",
                json!({"provider": entry.id, "source": source, "placeholder": value})
            );
        } else {
            let _ = writeln!(io.out, "{value}");
        }
    };
    for name in &entry.provider.env {
        if let Some(value) = ctx.env.var(name).filter(|v| !v.is_empty()) {
            return match crate::sealed::seal_value(sealed, name, value) {
                Ok(placeholder) => {
                    show(&placeholder, "env", io);
                    Exit::Ok
                }
                Err(e) => {
                    let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
                    Exit::Error
                }
            };
        }
    }
    for name in &entry.provider.env {
        if let Ok(reply) = sealed.request(&json!({"v": 1, "op": "placeholder", "name": name})) {
            if reply["state"] == "removed" {
                continue;
            }
            if let Some(value) = reply
                .get("placeholder")
                .and_then(Value::as_str)
                .filter(|v| crate::sealed::is_placeholder(v))
            {
                show(value, "sealed", io);
                return Exit::Ok;
            }
        }
    }
    let mut out = Vec::new();
    let mut err = Vec::new();
    let vault_args: Vec<String> = flags.bools.clone();
    let status = get(
        entry,
        &vault_args,
        None,
        sealed,
        &mut Io {
            out: &mut out,
            err: &mut err,
        },
    );
    if status == Exit::Ok {
        let _ = io.out.write_all(&out);
        return status;
    }
    let local = entry.provider.get.as_ref().is_some_and(|chain| {
        chain.iter().any(|verb| {
            verb.run.is_some()
                && verb
                    .requires
                    .as_deref()
                    .or(entry.provider.requires.as_deref())
                    .is_some_and(|name| crate::which::find(name, ctx.env, ctx.exists).is_some())
        })
    });
    if !local || status != Exit::SignIn {
        let _ = io.err.write_all(&err);
        return status;
    }
    let mut token = match crate::dispatch::fetch_from(entry, &vars, false, ctx, io) {
        Ok(token) => token,
        Err(code) => return code,
    };
    let Some(name) = entry.provider.env.first() else {
        let _ = writeln!(
            io.err,
            "jerk: {}: no environment name for sealing",
            entry.id
        );
        return Exit::Unsupported;
    };
    let result = token
        .fields
        .iter()
        .try_for_each(|(name, value)| crate::sealed::seal_value(sealed, name, value).map(|_| ()))
        .and_then(|_| crate::sealed::seal_value(sealed, name, &token.value));
    use zeroize::Zeroize;
    token.value.zeroize();
    for value in token.fields.values_mut() {
        value.zeroize();
    }
    match result {
        Ok(placeholder) => {
            show(&placeholder, token.source, io);
            Exit::Ok
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            Exit::Error
        }
    }
}

pub fn create(
    entry: &Entry,
    args: &[String],
    sealed: &dyn Sealed,
    sleep: &dyn Fn(u64),
    io: &mut Io<'_>,
) -> Exit {
    let mut scope: Option<&str> = None;
    let mut params = serde_json::Map::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--scope" => {
                scope = iter.next().map(String::as_str);
                if scope.is_none() {
                    let _ = writeln!(
                        io.err,
                        "jerk: {} create: --scope needs a value\nRun jerk --help for usage.",
                        entry.id
                    );
                    return Exit::Usage;
                }
            }
            "-p" | "--param" => {
                let pair = iter.next().map(String::as_str).unwrap_or("");
                match pair.split_once('=') {
                    Some((k, v)) => {
                        params.insert(k.to_string(), Value::String(v.to_string()));
                    }
                    None => {
                        let _ = writeln!(
                            io.err,
                            "jerk: {} create: -p needs key=value\nRun jerk --help for usage.",
                            entry.id
                        );
                        return Exit::Usage;
                    }
                }
            }
            other if other.starts_with('-') => {
                let _ = writeln!(
                    io.err,
                    "jerk: {} create: unexpected {other}\nRun jerk --help for usage.",
                    entry.id
                );
                return Exit::Usage;
            }
            _ => {}
        }
    }

    let is_host =
        entry.oauth_config.is_none() || entry.source.layer == crate::config::Layer::BuiltIn;
    let body = if is_host {
        let mut req = json!({
            "v": 1,
            "op": "request",
            "kind": "jerk",
            "jop": "create",
            "provider": entry.id,
        });
        if let Some(s) = scope {
            req["scope"] = json!(s);
        }
        if !params.is_empty() {
            req["params"] = Value::Object(params);
        }
        req
    } else {
        let mut oauth = serde_json::Map::new();
        if let Some(tbl) = &entry.provider.oauth {
            if let Ok(Value::Object(m)) = serde_json::to_value(tbl) {
                oauth = m;
            }
        }
        if let Some(s) = scope {
            oauth.insert("scope".to_string(), Value::String(s.to_string()));
        }
        json!({
            "v": 1,
            "op": "request",
            "kind": "oauth",
            "provider": entry.id,
            "oauth": oauth,
        })
    };

    let reply = match sealed.request(&body) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            return Exit::Error;
        }
    };

    if let Some(state) = reply.get("state").and_then(Value::as_str) {
        if state == "pending" {
            let id = reply
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            return poll_until_done(&entry.id, &id, is_host, sealed, sleep, io);
        }
    }

    finish_create(&entry.id, &reply, is_host, sealed, sleep, io)
}

fn poll_until_done(
    provider: &str,
    id: &str,
    is_host: bool,
    sealed: &dyn Sealed,
    sleep: &dyn Fn(u64),
    io: &mut Io<'_>,
) -> Exit {
    loop {
        sleep(1);
        match sealed.poll(id) {
            Ok(PollResult::Pending { progress }) => {
                for line in &progress {
                    let _ = writeln!(io.err, "{line}");
                }
            }
            Ok(PollResult::Done(reply)) => {
                return finish_create(provider, &reply, is_host, sealed, sleep, io);
            }
            Err(e) => {
                let _ = writeln!(io.err, "jerk: {provider}: poll failed: {e}");
                return Exit::Error;
            }
        }
    }
}

fn finish_create(
    provider: &str,
    reply: &Value,
    is_host: bool,
    sealed: &dyn Sealed,
    sleep: &dyn Fn(u64),
    io: &mut Io<'_>,
) -> Exit {
    if let Some(code) = refusal(reply) {
        let _ = writeln!(
            io.err,
            "jerk: {provider}: seven refused the request: {code}"
        );
        if is_mcp(provider) {
            hint_create(provider, io);
        }
        return Exit::Error;
    }
    let exit = reply.get("exit").and_then(Value::as_i64).unwrap_or(0);
    if exit == 0 && is_host {
        let waiting = crate::sealed::Waiting {
            inner: sealed,
            sleep,
            limit: 120,
        };
        let body = json!({ "v": 1, "op": "request", "kind": "jerk-seal", "provider": provider });
        let sealed_reply = match waiting.request(&body) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(
                    io.err,
                    "jerk: {provider}: signed in on the host, but sealing failed: {e}"
                );
                return Exit::Error;
            }
        };
        if let Some(code) = refusal(&sealed_reply) {
            let _ = writeln!(
                io.err,
                "jerk: {provider}: signed in on the host, but seven refused sealing: {code}"
            );
            return Exit::Error;
        }
        let code = sealed_reply
            .get("exit")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if code != 0 {
            let msg = sealed_reply
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("sealing failed");
            let _ = writeln!(io.err, "jerk: {provider}: signed in on the host, but {msg}");
            return exit_from(code);
        }
    }
    if exit != 0 {
        let msg = reply
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("failed");
        let _ = writeln!(io.err, "jerk: {provider}: {msg}");
        if is_mcp(provider) {
            hint_create(provider, io);
        }
        return exit_from(exit);
    }
    let user = reply.get("user").and_then(Value::as_str);
    match user {
        Some(u) => {
            let _ = writeln!(io.out, "{provider}: signed in as {u}");
        }
        None => {
            let _ = writeln!(io.out, "{provider}: signed in");
        }
    }
    Exit::Ok
}

pub fn drop_cmd(
    entry: &Entry,
    args: &[String],
    stdin_terminal: bool,
    sealed: &dyn Sealed,
    confirm: &mut dyn FnMut() -> bool,
    io: &mut Io<'_>,
) -> Exit {
    let yes = args.iter().any(|a| a == "--yes");
    for arg in args {
        if arg.starts_with('-') && arg != "--yes" {
            let _ = writeln!(
                io.err,
                "jerk: {} drop: unexpected {arg}\nRun jerk --help for usage.",
                entry.id
            );
            return Exit::Usage;
        }
    }

    if !yes {
        if !stdin_terminal {
            let _ = writeln!(
                io.err,
                "jerk: {} drop: pass --yes when not on a terminal\nRun jerk --help for usage.",
                entry.id
            );
            return Exit::Usage;
        }
        let _ = write!(io.err, "Drop {}? [y/N] ", entry.id);
        let _ = io.err.flush();
        if !confirm() {
            let _ = writeln!(io.err, "jerk: cancelled");
            return Exit::Error;
        }
    }

    let body = json!({
        "v": 1,
        "op": "request",
        "kind": "unseal",
        "provider": entry.id,
    });
    match sealed.request(&body) {
        Ok(reply) => {
            if let Some(code) = refusal(&reply) {
                let _ = writeln!(
                    io.err,
                    "jerk: {}: seven refused the request: {code}",
                    entry.id
                );
                return Exit::Error;
            }
            let exit = reply.get("exit").and_then(Value::as_i64).unwrap_or(0);
            if exit != 0 {
                let msg = reply
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("failed");
                let _ = writeln!(io.err, "jerk: {}: {msg}", entry.id);
                return exit_from(exit);
            }
            let _ = writeln!(io.out, "{}: dropped", entry.id);
            Exit::Ok
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            Exit::Error
        }
    }
}

fn refusal(reply: &Value) -> Option<&str> {
    let code = reply.get("error")?.get("code").and_then(Value::as_str);
    Some(
        code.filter(|c| c.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("unknown"),
    )
}

fn is_mcp(provider: &str) -> bool {
    provider.starts_with("mcp:")
}

fn hint_create(provider: &str, io: &mut Io<'_>) {
    if is_mcp(provider) {
        let _ = writeln!(
            io.err,
            "jerk: {provider}: sign-in happens in Settings › MCP servers, where it is sealed"
        );
        return;
    }
    let _ = writeln!(
        io.err,
        "jerk: {provider}: sign in with jerk {provider} create"
    );
}

fn exit_from(code: i64) -> Exit {
    match code {
        0 => Exit::Ok,
        2 => Exit::Usage,
        3 => Exit::SignIn,
        4 => Exit::Unavailable,
        5 => Exit::Unsupported,
        _ => Exit::Error,
    }
}
