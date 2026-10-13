use crate::config::{self, Config, Level};
use crate::dispatch;
use crate::exit::Exit;
use crate::paths::Env;
use crate::run::{Process, Runner};
use crate::which;
use serde_json::json;
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

pub(crate) const HELP: &str =
    "jerk: get, list, create and drop OAuth tokens through each provider's own CLI

Usage:
  jerk list [--json]
  jerk <provider> get    [--scope S] [--reveal] [--refresh] [--login] [--json]
  jerk <provider> list   [--json]
  jerk <provider> create [--scope S] [--force]
  jerk <provider> drop   [<id>] [--lease] [--yes]
  jerk <provider> lease
  jerk <provider> sync --fnox <SECRET> --provider <fnox provider>
  jerk config path [--json] | check | show <provider> | fnox
  jerk intercept --authorize-url <url> --redirect <url>

Options:
  --config <file>  read this providers file after the others (or SLICC_JERK_CONFIG)
  -h, --help       show this help
  -V, --version    show the version

Exit codes: 0 ok, 1 error, 2 usage, 3 sign-in needed, 4 not available here, 5 unsupported
";

pub(crate) const VERBS: [&str; 7] = [
    "get",
    "list",
    "create",
    "drop",
    "lease",
    "sync",
    "import-pi",
];

pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

pub struct Ctx<'a> {
    pub env: &'a Env,
    pub exists: &'a dyn Fn(&Path) -> bool,
    pub runner: &'a dyn Runner,
    pub now: u64,
    pub stdin_terminal: bool,
    pub stdout_terminal: bool,
    pub input: &'a mut dyn BufRead,
    pub random: &'a dyn Fn(&mut [u8]),
    pub oauth: &'a dyn crate::oauth::OAuthHost,
    pub via_link: bool,
    pub sealed: Option<&'a dyn crate::sealed::Sealed>,
    pub sleep: &'a dyn Fn(u64),
    pub seven_origin: Option<String>,
}

pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> Exit {
    let env = Env::from_process();
    let stdin = std::io::stdin();
    #[cfg(not(target_os = "wasi"))]
    let oauth_host = crate::oauth::DefaultOAuthHost;
    #[cfg(target_os = "wasi")]
    let oauth_host = crate::oauth::NoOAuthHost;
    #[cfg(target_os = "wasi")]
    let dev_sealed = crate::sealed::DevSealed;
    let mut ctx = Ctx {
        env: &env,
        exists: &which::is_file,
        runner: &Process,
        now: crate::time::now(),
        stdin_terminal: stdin.is_terminal(),
        stdout_terminal: std::io::stdout().is_terminal(),
        input: &mut stdin.lock(),
        random: &crate::fnox::os_random,
        oauth: &oauth_host,
        via_link: false,
        #[cfg(target_os = "wasi")]
        sealed: Some(&dev_sealed),
        #[cfg(not(target_os = "wasi"))]
        sealed: None,
        sleep: &|secs| std::thread::sleep(std::time::Duration::from_secs(secs)),
        seven_origin: None,
    };
    run_with(args, &mut ctx, &mut Io { out, err })
}

pub fn run_with(args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let env = ctx.env;
    let exists = ctx.exists;
    let (explicit, rest) = match split_config(args) {
        Ok(split) => split,
        Err(message) => return usage(&message, io),
    };
    let Some(first) = rest.first().map(String::as_str) else {
        let _ = io.out.write_all(HELP.as_bytes());
        return Exit::Ok;
    };
    match first {
        "-h" | "--help" | "help" => {
            let _ = io.out.write_all(HELP.as_bytes());
            Exit::Ok
        }
        "-V" | "--version" => {
            let _ = writeln!(io.out, "jerk {}", crate::version());
            Exit::Ok
        }
        "list" => {
            if let Some(sealed) = ctx.sealed {
                let config = config::load(env, explicit.as_deref());
                if let Some(code) = report(&config, io) {
                    return code;
                }
                let waiting = crate::sealed::Waiting {
                    inner: sealed,
                    sleep: ctx.sleep,
                    limit: 30,
                };
                return crate::kernel::list(&config, &rest[1..], &waiting, io);
            }
            list(&rest[1..], env, explicit.as_deref(), io, exists)
        }
        "config" => config_command(&rest[1..], env, explicit.as_deref(), io),
        "intercept" => intercept_command(&rest[1..], ctx, io),
        flag if flag.starts_with('-') => usage(&format!("unknown option {flag}"), io),
        provider => match rest.get(1).map(String::as_str) {
            Some(verb) if VERBS.contains(&verb) => {
                let config = config::load(env, explicit.as_deref());
                let linked = rest[2..].iter().any(|a| a == "--via-link");
                if linked {
                    if let Some(problem) = config.problems.iter().find(|p| p.level == Level::Error)
                    {
                        return crate::link::bare(provider, &problem.to_string(), io);
                    }
                    if config.get(provider).is_none() {
                        return crate::link::bare(
                            provider,
                            &format!("unknown provider {provider}"),
                            io,
                        );
                    }
                }
                if let Some(code) = report(&config, io) {
                    return code;
                }
                let args = &rest[2..];
                if let Some(sealed) = ctx.sealed {
                    let entry = config.get(provider).cloned();
                    return kernel_verb_or_passthrough(
                        provider,
                        entry.as_ref(),
                        verb,
                        args,
                        ctx,
                        sealed,
                        io,
                    );
                }
                let Some(entry) = config.get(provider) else {
                    return usage(
                        &format!(
                            "unknown provider {provider}; jerk list shows the configured ones"
                        ),
                        io,
                    );
                };
                if let Some((stripped, origin)) = crate::link::strip(args) {
                    ctx.seven_origin = origin;
                    return crate::link::run(entry, verb, &stripped, ctx, io);
                }
                if entry.provider.kind.as_deref() == Some("mcp") || provider.starts_with("mcp:") {
                    return match verb {
                        "get" => crate::mcp::get(entry, args, ctx, io),
                        "create" => crate::mcp::create(entry, args, ctx, io),
                        "drop" => crate::mcp::drop_cmd(entry, args, ctx, io),
                        "import-pi" => crate::mcp::import_pi(entry, args, ctx, io),
                        _ => usage(
                            &format!("{provider}: {verb} is not supported for MCP providers"),
                            io,
                        ),
                    };
                }
                match verb {
                    "get" => dispatch::get(entry, args, ctx, io),
                    "list" => dispatch::list(entry, args, ctx, io),
                    "create" => dispatch::create(entry, args, ctx, io),
                    "drop" => dispatch::drop(entry, args, ctx, io),
                    "lease" => crate::fnox::lease(entry, args, ctx, io),
                    _ => crate::fnox::sync(entry, args, ctx, io),
                }
            }
            Some(verb) => usage(&format!("unknown command {verb}"), io),
            None => usage(&format!("{provider}: missing a command"), io),
        },
    }
}

fn split_config(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut explicit = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !rest.is_empty() {
            rest.push(arg.clone());
        } else if arg == "--config" {
            match iter.next() {
                Some(value) if !value.starts_with('-') => explicit = Some(value.clone()),
                _ => return Err("--config needs a file".to_string()),
            }
        } else if let Some(value) = arg.strip_prefix("--config=") {
            if value.is_empty() {
                return Err("--config needs a file".to_string());
            }
            explicit = Some(value.to_string());
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((explicit, rest))
}

fn report(config: &Config, io: &mut Io<'_>) -> Option<Exit> {
    for problem in &config.problems {
        let _ = writeln!(io.err, "jerk: {problem}");
    }
    config.has_errors().then_some(Exit::Usage)
}

fn json_flag(args: &[String], what: &str, io: &mut Io<'_>) -> Result<bool, Exit> {
    match args {
        [] => Ok(false),
        [flag] if flag == "--json" => Ok(true),
        [other, ..] => Err(usage(&format!("{what}: unexpected {other}"), io)),
    }
}

fn list(
    args: &[String],
    env: &Env,
    explicit: Option<&str>,
    io: &mut Io<'_>,
    exists: &dyn Fn(&Path) -> bool,
) -> Exit {
    let json = match json_flag(args, "list", io) {
        Ok(json) => json,
        Err(code) => return code,
    };
    let config = config::load(env, explicit);
    if let Some(code) = report(&config, io) {
        return code;
    }
    let rows: Vec<[String; 4]> = config
        .providers
        .values()
        .map(|entry| {
            let cli = match &entry.provider.requires {
                Some(cmd) if which::find(cmd, env, exists).is_some() => format!("{cmd} (found)"),
                Some(cmd) => format!("{cmd} (missing)"),
                None => "-".to_string(),
            };
            [
                entry.id.clone(),
                entry
                    .provider
                    .name
                    .clone()
                    .unwrap_or_else(|| entry.id.clone()),
                entry.source.layer.label().to_string(),
                cli,
            ]
        })
        .collect();
    if json {
        let items: Vec<_> = config
            .providers
            .values()
            .zip(&rows)
            .map(|(entry, row)| {
                json!({
                    "provider": entry.id,
                    "name": row[1],
                    "source": row[2],
                    "requires": entry.provider.requires,
                    "cli": entry.provider.requires.as_ref().map(|_| row[3].ends_with("(found)")),
                    "env": entry.provider.env,
                })
            })
            .collect();
        let _ = writeln!(io.out, "{}", serde_json::Value::Array(items));
        return Exit::Ok;
    }
    let _ = io
        .out
        .write_all(table(&["PROVIDER", "NAME", "SOURCE", "CLI"], &rows).as_bytes());
    Exit::Ok
}

pub(crate) fn table(header: &[&str; 4], rows: &[[String; 4]]) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let mut text = String::new();
        for (i, cell) in cells.iter().enumerate() {
            if i + 1 == cells.len() {
                text.push_str(cell);
            } else {
                text.push_str(cell);
                text.push_str(&" ".repeat(widths[i] - cell.chars().count() + 2));
            }
        }
        text.push('\n');
        text
    };
    let mut text = line(header.to_vec());
    for row in rows {
        text.push_str(&line(row.iter().map(String::as_str).collect()));
    }
    text
}

fn config_command(args: &[String], env: &Env, explicit: Option<&str>, io: &mut Io<'_>) -> Exit {
    match args.first().map(String::as_str) {
        Some("path") => config_path(&args[1..], env, explicit, io),
        Some("check") if args.len() == 1 => config_check(env, explicit, io),
        Some("show") if args.len() == 2 => config_show(&args[1], env, explicit, io),
        Some("fnox") => {
            let config = config::load(env, explicit);
            if let Some(code) = report(&config, io) {
                return code;
            }
            crate::fnox::config_fnox(&args[1..], &config, io)
        }
        Some(other) if ["check", "show"].contains(&other) => {
            usage(&format!("config {other}: wrong arguments"), io)
        }
        Some(other) => usage(&format!("config: unknown command {other}"), io),
        None => usage("config needs path, check, show <provider> or fnox", io),
    }
}

fn config_path(args: &[String], env: &Env, explicit: Option<&str>, io: &mut Io<'_>) -> Exit {
    let json = match json_flag(args, "config path", io) {
        Ok(json) => json,
        Err(code) => return code,
    };
    let mut rows = vec![("system", env.system_dir())];
    rows.push(("user", env.user_file()));
    if let Some(path) = env.explicit_file(explicit) {
        rows.push(("explicit", path));
    }
    rows.push(("state", env.state_dir()));
    if json {
        let map: serde_json::Map<String, serde_json::Value> = rows
            .iter()
            .map(|(k, p)| {
                (
                    k.to_string(),
                    json!({ "path": p.display().to_string(), "exists": p.exists() }),
                )
            })
            .collect();
        let _ = writeln!(io.out, "{}", serde_json::Value::Object(map));
        return Exit::Ok;
    }
    for (label, path) in rows {
        let state = if path.exists() { "" } else { " (missing)" };
        let _ = writeln!(io.out, "{label:<9}{}{state}", path.display());
    }
    Exit::Ok
}

fn config_check(env: &Env, explicit: Option<&str>, io: &mut Io<'_>) -> Exit {
    let config = config::load(env, explicit);
    if let Some(code) = report(&config, io) {
        return code;
    }
    let mut counts = std::collections::BTreeMap::new();
    for entry in config.providers.values() {
        *counts.entry(entry.source.layer.clone()).or_insert(0) += 1;
    }
    let parts: Vec<String> = counts
        .iter()
        .map(|(layer, n)| format!("{n} {}", layer.label()))
        .collect();
    let warnings = config
        .problems
        .iter()
        .filter(|p| p.level == Level::Warning)
        .count();
    let suffix = if warnings > 0 {
        format!(
            ", {warnings} warning{}",
            if warnings == 1 { "" } else { "s" }
        )
    } else {
        String::new()
    };
    let _ = writeln!(
        io.out,
        "ok: {} providers ({}){suffix}",
        config.providers.len(),
        parts.join(", ")
    );
    Exit::Ok
}

fn config_show(id: &str, env: &Env, explicit: Option<&str>, io: &mut Io<'_>) -> Exit {
    let config = config::load(env, explicit);
    if let Some(code) = report(&config, io) {
        return code;
    }
    let Some(entry) = config.get(id) else {
        return usage(
            &format!("unknown provider {id}; jerk list shows the configured ones"),
            io,
        );
    };
    let mut providers = toml::Table::new();
    providers.insert(
        id.to_string(),
        toml::Value::try_from(&entry.provider).unwrap_or(toml::Value::Table(toml::Table::new())),
    );
    let mut root = toml::Table::new();
    root.insert("providers".to_string(), toml::Value::Table(providers));
    let _ = writeln!(io.err, "jerk: {id} comes from {}", entry.source);
    let _ = write!(io.out, "{root}");
    Exit::Ok
}

#[cfg(not(target_os = "wasi"))]
fn intercept_command(args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    if ctx.via_link {
        let _ = writeln!(io.err, "jerk intercept: not available over the link");
        return Exit::Unsupported;
    }
    if !ctx.stdin_terminal || !ctx.stdout_terminal {
        let _ = writeln!(
            io.err,
            "jerk intercept: stdin and stdout must both be terminals"
        );
        return Exit::Usage;
    }
    crate::intercept_cmd::run(args, ctx, io)
}

#[cfg(target_os = "wasi")]
fn intercept_command(_args: &[String], _ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let _ = writeln!(io.err, "jerk intercept: not available in this environment");
    Exit::Unsupported
}

#[allow(dead_code)]
fn not_yet(what: &str, io: &mut Io<'_>) -> Exit {
    let _ = writeln!(io.err, "jerk {what}: not implemented yet");
    Exit::Unsupported
}

pub(crate) fn usage(message: &str, io: &mut Io<'_>) -> Exit {
    let _ = writeln!(io.err, "jerk: {message}\nRun jerk --help for usage.");
    Exit::Usage
}

fn kernel_verb_or_passthrough(
    provider: &str,
    entry: Option<&crate::config::Entry>,
    verb: &str,
    args: &[String],
    ctx: &mut Ctx<'_>,
    sealed: &dyn crate::sealed::Sealed,
    io: &mut Io<'_>,
) -> Exit {
    if let Some(entry) = entry {
        return kernel_verb(entry, verb, args, ctx, sealed, io);
    }
    let waiting = crate::sealed::Waiting {
        inner: sealed,
        sleep: ctx.sleep,
        limit: 30,
    };
    let rows = crate::kernel::host_list(&waiting).unwrap_or_default();
    let row = rows.into_iter().find(|r| r.provider == provider);
    let Some(row) = row else {
        return usage(
            &format!("unknown provider {provider}; jerk list shows the configured ones"),
            io,
        );
    };
    let stub = crate::config::Entry {
        id: row.provider.clone(),
        provider: crate::config::Provider {
            name: Some(row.name.clone()),
            env: row.env.clone(),
            ..crate::config::Provider::default()
        },
        source: crate::config::Source {
            layer: crate::config::Layer::BuiltIn,
            path: None,
        },
        oauth_config: None,
    };
    kernel_verb(&stub, verb, args, ctx, sealed, io)
}

fn kernel_verb(
    entry: &crate::config::Entry,
    verb: &str,
    args: &[String],
    ctx: &mut Ctx<'_>,
    sealed: &dyn crate::sealed::Sealed,
    io: &mut Io<'_>,
) -> Exit {
    match verb {
        "get" => {
            let env_val = entry
                .provider
                .env
                .first()
                .and_then(|n| ctx.env.var(n))
                .map(String::from);
            let waiting = crate::sealed::Waiting {
                inner: sealed,
                sleep: ctx.sleep,
                limit: 30,
            };
            crate::kernel::get(entry, args, env_val.as_deref(), &waiting, io)
        }
        "create" => crate::kernel::create(entry, args, sealed, ctx.sleep, io),
        "drop" => {
            let stdin_terminal = ctx.stdin_terminal;
            let input = &mut *ctx.input;
            let mut confirm = || {
                let mut answer = String::new();
                let _ = input.read_line(&mut answer);
                matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
            };
            let waiting = crate::sealed::Waiting {
                inner: sealed,
                sleep: ctx.sleep,
                limit: 120,
            };
            crate::kernel::drop_cmd(entry, args, stdin_terminal, &waiting, &mut confirm, io)
        }
        "lease" => {
            let _ = writeln!(
                io.err,
                "jerk {}: lease is not supported in the kernel",
                entry.id
            );
            Exit::Unsupported
        }
        "sync" => {
            let _ = writeln!(
                io.err,
                "jerk {}: sync is not supported in the kernel",
                entry.id
            );
            Exit::Unsupported
        }
        "list" => {
            let _ = writeln!(
                io.err,
                "jerk {}: per-provider list is not supported in the kernel",
                entry.id
            );
            Exit::Unsupported
        }
        _ => usage(&format!("unknown command {verb}"), io),
    }
}
