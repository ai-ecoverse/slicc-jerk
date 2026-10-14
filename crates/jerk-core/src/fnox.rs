use crate::cli::{usage, Ctx, Io};
use crate::config::{valid_env, Config, Entry};
use crate::dispatch::{fetch, mint_token, parse_flags, pick, run_verb, vars, Flags};
use crate::exit::Exit;
use crate::run::{Opts, Outcome};
use crate::{state, time, which};
use serde_json::json;
use std::fmt::Write as _;

pub const ENCRYPTING: [&str; 9] = [
    "age",
    "keychain",
    "1password",
    "bitwarden",
    "fido2",
    "yubikey",
    "aws-kms",
    "azure-kms",
    "gcp-kms",
];

const DEFAULT_DURATION: u64 = 3600;

pub fn os_random(buf: &mut [u8]) {
    let _ = getrandom::fill(buf);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn lease(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &[], &[], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} lease: {e}", entry.id), io),
    };
    if ctx.stdout_terminal {
        return usage(
            &format!(
                "{} lease is for fnox and refuses a terminal; use jerk {} get",
                entry.id, entry.id
            ),
            io,
        );
    }
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    let duration = ctx
        .env
        .var("FNOX_LEASE_DURATION")
        .and_then(|d| d.parse::<u64>().ok())
        .filter(|d| *d > 0)
        .unwrap_or(DEFAULT_DURATION);
    let minting = entry.provider.lease_via.as_deref() == Some("create");
    let minter = match (&entry.provider.create, minting) {
        (Some(chain), true) => match pick(entry, "create", chain, ctx, io) {
            Ok(verb) => Some(verb).filter(|v| v.parse.is_some()),
            Err(code) => return code,
        },
        _ => None,
    };
    let (value, upstream, expires_at) = match minter {
        Some(verb) => match mint_token(entry, &verb, &vars, false, ctx, io) {
            Ok((value, id)) => (value, id, None),
            Err(code) => return code,
        },
        None => match fetch(entry, &vars, ctx, io) {
            Ok(token) => (token.value, None, token.expires_at),
            Err(code) => return code,
        },
    };
    let mut random = [0u8; 8];
    (ctx.random)(&mut random);
    let lease_id = format!("jerk-{}-{}", entry.id, hex(&random));
    match (&upstream, minting) {
        (Some(id), _) => {
            if let Err(e) = state::lease_record(ctx.env, &lease_id, &entry.id, id) {
                let _ = writeln!(
                    io.err,
                    "jerk: {}: cannot record the lease, so no lease is given: {e}; revoke token {id} by hand",
                    entry.id
                );
                return Exit::Error;
            }
        }
        (None, true) => {
            let _ = writeln!(
                io.err,
                "jerk: {}: minted a token whose id jerk could not find, so no lease is given; revoke it by hand",
                entry.id
            );
            return Exit::Error;
        }
        (None, false) => {}
    }
    let until = (ctx.now + duration).min(expires_at.unwrap_or(u64::MAX));
    let credentials: serde_json::Map<String, serde_json::Value> = entry
        .provider
        .env
        .iter()
        .map(|name| (name.clone(), json!(value)))
        .collect();
    let doc = json!({
        "credentials": credentials,
        "expires_at": time::rfc3339(until),
        "lease_id": lease_id,
    });
    let _ = writeln!(io.out, "{doc}");
    Exit::Ok
}

pub fn drop_lease(entry: &Entry, flags: &Flags, ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    if !flags.positionals.is_empty() {
        return usage(
            &format!(
                "{} drop --lease takes no id; fnox sets FNOX_LEASE_ID",
                entry.id
            ),
            io,
        );
    }
    let Some(lease_id) = ctx.env.var("FNOX_LEASE_ID").map(String::from) else {
        return usage(
            &format!("{} drop --lease needs FNOX_LEASE_ID from fnox", entry.id),
            io,
        );
    };
    let found = state::lease_find(ctx.env, &lease_id).filter(|(provider, _)| *provider == entry.id);
    let Some((_, id)) = found else {
        let _ = writeln!(
            io.err,
            "jerk: {}: lease {lease_id} holds a token jerk did not mint; it lives on until it expires or the CLI signs out",
            entry.id
        );
        return Exit::Ok;
    };
    let step = entry
        .provider
        .drop
        .as_ref()
        .map(|chain| pick(entry, "drop", chain, ctx, io));
    let verb = match step {
        Some(Ok(verb)) if !verb.oauth => verb,
        Some(Err(code)) => return code,
        _ => {
            let _ = writeln!(io.err, "jerk: {} has no drop command", entry.id);
            return Exit::Unsupported;
        }
    };
    let mut lease_flags = Flags::default();
    lease_flags.positionals.push(id.clone());
    let vars = vars(entry, &lease_flags).unwrap_or_default();
    if let Err(code) = run_verb(entry, "drop", &verb, &vars, false, ctx, io) {
        return code;
    }
    let _ = state::forget(ctx.env, &entry.id, &id);
    let _ = state::lease_forget(ctx.env, &lease_id);
    let _ = writeln!(
        io.err,
        "jerk: {}: revoked the token behind {lease_id}",
        entry.id
    );
    Exit::Ok
}

pub fn config_fnox(args: &[String], config: &Config, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &[], &["--proxy"], usize::MAX) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("config fnox: {e}"), io),
    };
    for id in &flags.positionals {
        if config.get(id).is_none() {
            return usage(
                &format!("unknown provider {id}; jerk list shows the configured ones"),
                io,
            );
        }
    }
    let leasable = |entry: &&Entry| {
        entry.provider.get.is_some()
            || entry
                .provider
                .create
                .as_ref()
                .is_some_and(|c| c.iter().any(|v| v.parse.is_some()))
    };
    let entries: Vec<&Entry> = config
        .providers
        .values()
        .filter(|e| flags.positionals.is_empty() || flags.positionals.contains(&e.id))
        .filter(leasable)
        .collect();
    let mut text = String::new();
    for entry in &entries {
        let _ = writeln!(
            text,
            "[leases.{id}]\ntype = \"command\"\ncreate_command = \"jerk {id} lease\"\nrevoke_command = \"jerk {id} drop --lease\"\nduration = \"1h\"\n",
            id = entry.id
        );
    }
    if flags.has("--proxy") {
        for entry in &entries {
            for domain in &entry.provider.domains {
                let _ = writeln!(
                    text,
                    "[[proxy.rules]]\nsecret = \"{}\"\ndomain = \"{domain}\"\nheader = \"authorization\"\n",
                    entry.provider.env[0]
                );
            }
        }
        for entry in &entries {
            let _ = writeln!(
                io.err,
                "jerk: fnox proxy reads the secret {}; keep it current with jerk {} sync --fnox {} --provider <fnox provider>",
                entry.provider.env[0], entry.id, entry.provider.env[0]
            );
        }
    }
    let _ = write!(io.out, "{}", text.trim_end_matches('\n'));
    let _ = writeln!(io.out);
    Exit::Ok
}

fn provider_types(paths: &str, name: &str) -> Vec<String> {
    let kind = |provider: &toml::Value| {
        provider
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    };
    let mut types = Vec::new();
    for doc in paths
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .filter_map(|text| text.parse::<toml::Table>().ok())
    {
        if let Some(provider) = doc.get("providers").and_then(|p| p.get(name)) {
            types.push(kind(provider));
        }
        let profiles = doc.get("profiles").and_then(toml::Value::as_table);
        for profile in profiles.into_iter().flat_map(|p| p.values()) {
            if let Some(provider) = profile.get("providers").and_then(|p| p.get(name)) {
                types.push(kind(provider));
            }
        }
    }
    types
}

fn run_fnox(
    entry: &Entry,
    fnox: &std::path::Path,
    args: &[String],
    input: Option<&str>,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Result<String, Exit> {
    let opts = Opts {
        interactive: false,
        capture: true,
        timeout: Some(std::time::Duration::from_secs(60)),
        remove_env: &entry.provider.env,
        input,
        on_stderr: None,
    };
    match ctx.runner.run(fnox, args, &opts) {
        Outcome::Exited {
            code: 0, stdout, ..
        } => Ok(stdout),
        Outcome::Exited { code, .. } => {
            let _ = writeln!(io.err, "jerk: fnox {} exited {code}", args[0]);
            Err(Exit::Error)
        }
        Outcome::TimedOut => {
            let _ = writeln!(io.err, "jerk: fnox {} did not answer in time", args[0]);
            Err(Exit::Error)
        }
        Outcome::Failed(e) => {
            let _ = writeln!(io.err, "jerk: cannot run fnox: {e}");
            Err(Exit::Unavailable)
        }
    }
}

pub fn sync(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match parse_flags(args, &["--fnox", "--provider"], &["--global"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} sync: {e}", entry.id), io),
    };
    let (Some(secret), Some(provider)) = (flags.value("--fnox"), flags.value("--provider")) else {
        return usage(
            &format!(
                "{} sync needs --fnox <SECRET> and --provider <fnox provider>",
                entry.id
            ),
            io,
        );
    };
    if !valid_env(secret) {
        return usage(&format!("{secret} is not a valid fnox secret name"), io);
    }
    let Some(fnox) = which::find("fnox", ctx.env, ctx.exists) else {
        let _ = writeln!(io.err, "jerk: fnox is not on PATH; install it first");
        return Exit::Unavailable;
    };
    let files = match run_fnox(entry, &fnox, &["config-files".to_string()], None, ctx, io) {
        Ok(files) => files,
        Err(code) => return code,
    };
    let types = provider_types(&files, provider);
    if types.is_empty() {
        return usage(&format!("fnox has no provider {provider}"), io);
    }
    if let Some(bad) = types.iter().find(|t| !ENCRYPTING.contains(&t.as_str())) {
        let kind = if bad.is_empty() {
            "an unknown type"
        } else {
            bad.as_str()
        };
        return usage(
            &format!(
                "fnox provider {provider} is {kind}; sync only writes to one that encrypts or uses an OS store ({})",
                ENCRYPTING.join(", ")
            ),
            io,
        );
    }
    let vars = match vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return usage(&e, io),
    };
    let token = match fetch(entry, &vars, ctx, io) {
        Ok(token) => token,
        Err(code) => return code,
    };
    let mut set = vec![
        "set".to_string(),
        secret.to_string(),
        "--provider".to_string(),
        provider.to_string(),
    ];
    if flags.has("--global") {
        set.push("--global".to_string());
    }
    if let Err(code) = run_fnox(entry, &fnox, &set, Some(&token.value), ctx, io) {
        return code;
    }
    if let Err(e) = state::touch_synced(ctx.env, ctx.now) {
        let _ = writeln!(io.err, "jerk: cannot touch the sync stamp: {e}");
    }
    let _ = writeln!(
        io.out,
        "{}: synced into fnox {secret} ({provider})",
        entry.id
    );
    Exit::Ok
}
