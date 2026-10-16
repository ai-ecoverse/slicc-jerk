use crate::cli::{usage, Ctx, Io};
use crate::dispatch::parse_flags;
use crate::exit::Exit;
use crate::oauth::{self, CdpOpener};
use jerk_oauth::config::{Capture, CaptureAt, CaptureFrom, Flow, Mode, OAuthConfig, Rewrite};

pub fn run(args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let rewrites = parse_rewrites(args);
    let filtered: Vec<String> = filter_rewrites(args);
    let flags = match parse_flags(
        &filtered,
        &[
            "--authorize-url",
            "--redirect",
            "--token-url",
            "--client-id",
            "--cdp",
            "--from-file",
            "--state",
        ],
        &["--pkce", "--exchange"],
        0,
    ) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("intercept: {e}"), io),
    };

    let config = if let Some(path) = flags.value("--from-file") {
        match load_from_file(path) {
            Ok(config) => config,
            Err(e) => {
                let _ = writeln!(io.err, "jerk intercept: {e}");
                return Exit::Usage;
            }
        }
    } else {
        let authorize_url = match flags.value("--authorize-url") {
            Some(url) => url,
            None => return usage("intercept: --authorize-url is required", io),
        };
        let redirect_uri = match flags.value("--redirect") {
            Some(url) => url,
            None => return usage("intercept: --redirect is required", io),
        };

        let has_challenge = authorize_url.contains("{code_challenge}");
        let pkce = flags.has("--pkce") || has_challenge;
        let exchange = flags.has("--exchange");

        let flow = if pkce {
            Flow::Pkce
        } else if exchange {
            Flow::Code
        } else {
            Flow::Implicit
        };

        let token_url = flags.value("--token-url").map(String::from);
        let client_id = flags
            .value("--client-id")
            .unwrap_or("jerk-intercept")
            .to_string();

        OAuthConfig {
            mode: Mode::Intercept,
            flow,
            authorize_url: authorize_url.to_string(),
            token_url,
            client_id,
            client_secret: None,
            redirect_uri: redirect_uri.to_string(),
            scope: None,
            capture: Capture {
                at: CaptureAt::Request,
                from: CaptureFrom::Query,
                param: "code".to_string(),
                url: None,
                header: None,
                stage: None,
                fields: std::collections::BTreeMap::new(),
            },
            relay: None,
            rewrite: rewrites,
            on_capture: None,
            timeout: Some("2m".to_string()),
            expires_param: None,
            renew: None,
            isolate: None,
            revoke_url: None,
            refresh_env: None,
            exchange: None,
        }
    };

    let cdp_url = flags.value("--cdp");
    let host = ctx.oauth;

    struct Opener<'a> {
        cdp_url: Option<&'a str>,
        env: &'a crate::paths::Env,
        runner: &'a dyn crate::run::Runner,
        exists: &'a dyn Fn(&std::path::Path) -> bool,
    }
    impl oauth::CdpOpener for Opener<'_> {
        fn open(&self) -> Result<oauth::CdpResult, String> {
            let ws_url = crate::browser_select::resolve_cdp_url(
                self.cdp_url,
                None,
                self.env,
                self.runner,
                self.exists,
                &mut std::io::sink(),
            )
            .map_err(|e| e.message)?;
            let cdp = cdp_client::cdp::Cdp::connect(&ws_url)?;
            Ok(oauth::CdpResult(cdp))
        }
    }
    let opener = Opener {
        cdp_url,
        env: ctx.env,
        runner: ctx.runner,
        exists: ctx.exists,
    };

    let exchange = flags.has("--exchange");

    if exchange {
        if matches!(config.flow, Flow::Pkce | Flow::Code) && config.token_url.is_none() {
            return usage(
                "intercept: --token-url is required for pkce and code exchange",
                io,
            );
        }
        let token = match oauth::oauth_create(
            &config,
            None,
            host,
            ctx.now,
            ctx.random,
            io.err,
            Some(&opener),
        ) {
            Ok(t) => t,
            Err(e) => {
                let _ = writeln!(io.err, "jerk intercept: {e}");
                return e.exit;
            }
        };
        let _ = writeln!(io.out, "{}", token.access_token);
    } else {
        let mut r32 = [0u8; 32];
        let mut r16 = [0u8; 16];
        (ctx.random)(&mut r32);
        (ctx.random)(&mut r16);
        let cdp_result = opener.open();
        let oauth::CdpResult(mut cdp) = match cdp_result {
            Ok(c) => c,
            Err(e) => {
                let _ = writeln!(io.err, "jerk intercept: {e}");
                return Exit::Unavailable;
            }
        };
        let mut capture_config = config.clone();
        capture_config.flow = Flow::Implicit;
        capture_config.token_url = None;
        match crate::oauth::intercept::run(
            &mut cdp,
            &capture_config,
            None,
            ctx.now,
            &r32,
            &r16,
            &|_, _| Err("no exchange".into()),
            io.err,
        ) {
            Ok(token) => {
                let _ = writeln!(
                    io.err,
                    "jerk intercept: captured (not exchanged, not stored)"
                );
                let _ = writeln!(io.out, "{}", token.access_token);
            }
            Err(e) => {
                let _ = writeln!(io.err, "jerk intercept: {e}");
                return e.exit;
            }
        }
    };
    Exit::Ok
}

fn filter_rewrites(args: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--rewrite" {
            iter.next();
        } else if arg.starts_with("--rewrite=") {
            continue;
        } else {
            result.push(arg.clone());
        }
    }
    result
}

fn parse_rewrites(args: &[String]) -> Vec<Rewrite> {
    let mut rewrites = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--rewrite" {
            if let Some(value) = iter.next() {
                if let Some(rule) = parse_rewrite_value(value) {
                    rewrites.push(rule);
                }
            }
        } else if let Some(value) = arg.strip_prefix("--rewrite=") {
            if let Some(rule) = parse_rewrite_value(value) {
                rewrites.push(rule);
            }
        }
    }
    rewrites
}

fn parse_rewrite_value(value: &str) -> Option<Rewrite> {
    let mut parts = value.splitn(2, '=');
    let match_pattern = parts.next()?.to_string();
    let rest = parts.next()?;
    let mut kv_parts = rest.splitn(2, '=');
    let key = kv_parts.next()?.to_string();
    let val = kv_parts.next()?.to_string();
    let mut append = std::collections::BTreeMap::new();
    append.insert(key, val);
    Some(Rewrite {
        match_pattern,
        append: Some(append),
        replace_url: None,
    })
}

fn load_from_file(path: &str) -> Result<OAuthConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let table: toml::Table =
        toml::from_str(&text).map_err(|e| format!("invalid TOML in {path}: {e}"))?;
    let oauth = table
        .get("oauth")
        .ok_or_else(|| format!("{path}: missing [oauth] section"))?;
    let config: OAuthConfig = toml::Value::Table(oauth.as_table().cloned().unwrap_or_default())
        .try_into()
        .map_err(|e| format!("{path}: invalid oauth config: {e}"))?;
    Ok(config)
}
