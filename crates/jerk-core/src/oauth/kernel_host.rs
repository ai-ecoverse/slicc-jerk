use super::{McpProbeResult, OAuthHost, Token};
use crate::browser::Browser;
use crate::grants::{Grant, GrantError, Grants};
use crate::sealed::{DevSealed, Sealed};
use serde_json::json;
use zeroize::Zeroize;

#[derive(Default)]
pub struct KernelOAuthHost {
    confined: bool,
}

impl Browser for KernelOAuthHost {
    fn open_url(&self, url: &str) -> Result<(), String> {
        let endpoint =
            std::env::var("SLICC_CDP_URL").map_err(|_| "SLICC_CDP_URL is not set".to_string())?;
        let mut cdp = cdp_client::cdp::Cdp::connect(&endpoint)?;
        cdp.call("Target.createTarget", json!({"url": url}), None)?;
        Ok(())
    }
}

impl OAuthHost for KernelOAuthHost {
    fn browser(&self) -> &dyn Browser {
        self
    }
    fn grants(&self, _: Option<&str>) -> Box<dyn Grants> {
        Box::new(PlaceholderStore(crate::grants::file_store::FileStore::new(
            crate::paths::Env::from_process().state_dir(),
        )))
    }
    fn http_post(&self, url: &str, body: &str) -> Result<String, String> {
        self.reachable(url)?;
        super::kernel_http::request("POST", url, body, "application/x-www-form-urlencoded")
            .map(|r| r.body)
    }
    fn http_post_json(&self, url: &str, body: &str) -> Result<String, String> {
        self.reachable(url)?;
        super::kernel_http::request("POST", url, body, "application/json").map(|r| r.body)
    }
    fn http_get(&self, url: &str) -> Result<String, String> {
        self.reachable(url)?;
        super::kernel_http::request("GET", url, "", "application/json").map(|r| r.body)
    }
    fn mcp_probe(&self, url: &str, body: &str) -> McpProbeResult {
        self.reachable(url)
            .and_then(|_| super::kernel_http::request("POST", url, body, "application/json"))
            .unwrap_or(McpProbeResult {
                status: 0,
                www_authenticate: None,
                body: String::new(),
            })
    }
    fn sleep(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    fn confine(&self, _: crate::reach::Reach) -> Box<dyn OAuthHost> {
        Box::new(Self { confined: true })
    }
    fn reachable(&self, _: &str) -> Result<(), String> {
        if self.confined {
            Err(crate::reach::REFUSED.into())
        } else {
            Ok(())
        }
    }
}

struct PlaceholderStore(crate::grants::file_store::FileStore);

impl Grants for PlaceholderStore {
    fn get(&self, provider: &str, user: &str) -> Option<Grant> {
        self.0.get(provider, user).filter(safe_grant)
    }
    fn store(&self, grant: &Grant) -> Result<(), GrantError> {
        if !safe_grant(grant) {
            return Err(GrantError::Backend("kernel grants must be sealed".into()));
        }
        self.0.store(grant)
    }
    fn remove(&self, provider: &str, user: &str) -> Result<(), GrantError> {
        self.0.remove(provider, user)
    }
}

fn safe_grant(grant: &Grant) -> bool {
    grant.refresh_token.is_empty()
        && grant.access_token.as_deref().is_some_and(placeholder)
        && grant.fields.values().all(|v| placeholder(v))
}

fn placeholder(value: &str) -> bool {
    value
        .strip_prefix("fnox_proxy_")
        .is_some_and(|v| v.len() == 24 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn seal(name: &str, value: &str) -> Result<String, String> {
    if placeholder(value) {
        return Ok(value.to_string());
    }
    if let Ok(record) = DevSealed.request(&json!({"v": 1, "op": "placeholder", "name": name})) {
        if record["state"] != "removed" && record["placeholder"].as_str() == Some(value) {
            return Ok(value.to_string());
        }
    }
    let reply = DevSealed
        .request(&json!({"v": 1, "op": "seal", "name": name, "value": value}))
        .map_err(|_| "cannot seal OAuth token".to_string())?;
    if reply.get("error").is_some() {
        let code = reply["error"]["code"]
            .as_str()
            .filter(|s| s.bytes().all(|b| b.is_ascii_uppercase()))
            .unwrap_or("EIO");
        return Err(format!("seven refused sealing: {code}"));
    }
    reply["placeholder"]
        .as_str()
        .filter(|s| placeholder(s))
        .map(String::from)
        .ok_or_else(|| "sealing returned no placeholder".into())
}

pub fn get(
    entry: &crate::config::Entry,
    args: &[String],
    ctx: &mut crate::cli::Ctx<'_>,
    io: &mut crate::cli::Io<'_>,
) -> crate::exit::Exit {
    use crate::exit::Exit;
    let flags = match crate::dispatch::parse_flags(args, &[], &["--json", "--reveal"], 0) {
        Ok(flags) => flags,
        Err(e) => return crate::cli::usage(&format!("{} get: {e}", entry.id), io),
    };
    let vars = match crate::dispatch::vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return crate::cli::usage(&e, io),
    };
    let user = crate::dispatch::grant_user(&vars);
    let cached = ctx
        .oauth
        .grants(entry.provider.store.as_deref())
        .get(&entry.id, user);
    let has_env = entry
        .provider
        .env
        .iter()
        .any(|name| ctx.env.var(name).is_some());
    if !has_env {
        for name in &entry.provider.env {
            let Ok(record) = DevSealed.request(&json!({"v": 1, "op": "placeholder", "name": name}))
            else {
                continue;
            };
            if record["state"] == "removed" {
                continue;
            }
            let Some(value) = record["placeholder"]
                .as_str()
                .filter(|value| placeholder(value))
            else {
                continue;
            };
            let same = cached.as_ref().and_then(|g| g.access_token.as_deref()) == Some(value);
            if same
                && cached
                    .as_ref()
                    .and_then(|g| g.expires_at)
                    .is_some_and(|expiry| ctx.now.saturating_add(300) >= expiry)
            {
                continue;
            }
            let source = if same { "oauth" } else { "sealed" };
            if flags.has("--json") {
                let _ = writeln!(
                    io.out,
                    "{}",
                    json!({"provider": entry.id, "source": source, "placeholder": value})
                );
            } else {
                let _ = writeln!(io.out, "{value}");
            }
            return Exit::Ok;
        }
    }
    if !has_env && cached.is_none() {
        let waiting = crate::sealed::Waiting {
            inner: &DevSealed,
            sleep: ctx.sleep,
            limit: 30,
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let status = crate::kernel::get(
            entry,
            &flags.bools,
            None,
            &waiting,
            &mut crate::cli::Io {
                out: &mut out,
                err: &mut err,
            },
        );
        if status == Exit::Ok {
            let _ = io.out.write_all(&out);
            return status;
        }
        if !matches!(status, Exit::SignIn | Exit::Unavailable) {
            let _ = io.err.write_all(&err);
            return status;
        }
    }
    let mut token = match crate::dispatch::fetch(entry, &vars, ctx, io) {
        Ok(token) => token,
        Err(code) => return code,
    };
    let result = entry
        .provider
        .env
        .first()
        .ok_or_else(|| "OAuth provider needs an environment name for sealing".to_string())
        .and_then(|name| seal(name, &token.value));
    token.value.zeroize();
    match result {
        Ok(placeholder) => {
            if flags.has("--json") {
                let _ = writeln!(
                    io.out,
                    "{}",
                    json!({"provider": entry.id, "source": token.source, "placeholder": placeholder})
                );
            } else {
                let _ = writeln!(io.out, "{placeholder}");
            }
            Exit::Ok
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: {e}", entry.id);
            Exit::Error
        }
    }
}

pub fn drop(
    entry: &crate::config::Entry,
    args: &[String],
    ctx: &mut crate::cli::Ctx<'_>,
    io: &mut crate::cli::Io<'_>,
) -> crate::exit::Exit {
    use crate::exit::Exit;
    let flags = match crate::dispatch::parse_flags(args, &[], &["--yes"], 0) {
        Ok(flags) => flags,
        Err(e) => return crate::cli::usage(&format!("{} drop: {e}", entry.id), io),
    };
    let vars = match crate::dispatch::vars(entry, &flags) {
        Ok(vars) => vars,
        Err(e) => return crate::cli::usage(&e, io),
    };
    let mut confirm = || {
        let mut answer = String::new();
        let _ = ctx.input.read_line(&mut answer);
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    };
    let waiting = crate::sealed::Waiting {
        inner: &DevSealed,
        sleep: ctx.sleep,
        limit: 120,
    };
    let mut out = Vec::new();
    let status = crate::kernel::drop_cmd(
        entry,
        &flags.bools,
        ctx.stdin_terminal,
        &waiting,
        &mut confirm,
        &mut crate::cli::Io {
            out: &mut out,
            err: io.err,
        },
    );
    if status != Exit::Ok {
        return status;
    }
    if let Err(e) = ctx
        .oauth
        .grants(entry.provider.store.as_deref())
        .remove(&entry.id, crate::dispatch::grant_user(&vars))
    {
        let _ = writeln!(io.err, "jerk: {}: cannot remove grant: {e}", entry.id);
        return Exit::Error;
    }
    let _ = io.out.write_all(&out);
    Exit::Ok
}

pub fn seal_token(entry: &crate::config::Entry, mut token: Token) -> Result<Token, String> {
    let result = (|| {
        let name = entry
            .provider
            .env
            .first()
            .ok_or("OAuth provider needs an environment name for sealing")?;
        let access_token = seal(name, &token.access_token)?;
        let fields = token
            .fields
            .iter()
            .map(|(name, value)| seal(name, value).map(|p| (name.clone(), p)))
            .collect::<Result<_, _>>()?;
        Ok(Token {
            access_token,
            refresh_token: None,
            expires_at: token.expires_at,
            scope: token.scope.as_ref().map(|scope| {
                let mut scope = scope.replace(&token.access_token, "[redacted]");
                if let Some(refresh) = token.refresh_token.as_ref().filter(|r| !r.is_empty()) {
                    scope = scope.replace(refresh, "[redacted]");
                }
                scope
            }),
            token_type: None,
            fields,
        })
    })();
    token.access_token.zeroize();
    token.refresh_token.zeroize();
    for value in token.fields.values_mut() {
        value.zeroize();
    }
    result
}
