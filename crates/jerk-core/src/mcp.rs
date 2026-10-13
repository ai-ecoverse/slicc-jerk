use crate::cli::{usage, Ctx, Io};
use crate::config::Entry;
use crate::exit::Exit;
use crate::mcp_config::{info_from_entry, McpProviderInfo};
#[allow(unused_imports)]
use std::io::Write;

pub fn get(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match crate::dispatch::parse_flags(args, &["--scope"], &["--reveal", "--json"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} get: {e}", entry.id), io),
    };
    let info = match info_from_entry(entry) {
        Some(i) => i,
        None => return err(io, &entry.id, "missing url"),
    };
    if let Some(value) = entry.provider.env.first().and_then(|n| ctx.env.var(n)) {
        return emit_token(value, "env", None, None, &flags, &entry.id, io);
    }
    let host = ctx.oauth;
    let grants = host.grants(entry.provider.store.as_deref());
    let account = grant_account(&info);
    let grant = match grants.get(&entry.id, &account) {
        Some(g) => g,
        None => return sign_in(io, &entry.id),
    };
    let requested = flags.value("--scope");
    if let Some(at) = &grant.access_token {
        let fresh = grant
            .expires_at
            .map(|exp| ctx.now + 300 < exp)
            .unwrap_or(true);
        if fresh {
            if requested
                .filter(|req| !jerk_oauth::scopes::scopes_satisfied(grant.scope.as_deref(), req))
                .is_some()
            {
                return scope_hint(io, &entry.id, requested.unwrap());
            }
            return emit_token(
                at,
                "oauth",
                grant.expires_at.map(crate::time::rfc3339).as_deref(),
                grant.scope.as_deref(),
                &flags,
                &entry.id,
                io,
            );
        }
    }
    if grant.refresh_token.is_empty() {
        let _ = writeln!(
            io.err,
            "jerk: {}: token expired; run jerk {} create",
            entry.id, entry.id
        );
        return Exit::SignIn;
    }
    let (_prm, as_meta) = match discover_oauth(&info, host, io, &entry.id) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let client_id = load_client_id(&info, ctx.env, host).unwrap_or_default();
    let mut body = vec![
        ("grant_type".into(), "refresh_token".into()),
        ("refresh_token".into(), grant.refresh_token.clone()),
        ("client_id".into(), client_id.clone()),
    ];
    if let Ok(r) = jerk_oauth::discovery::resource_url(&info.url) {
        body.push(("resource".into(), r));
    }
    let form = jerk_oauth::exchange::encode_form_body(&body);
    match host.http_post(&as_meta.token_endpoint, &form) {
        Ok(resp) => match jerk_oauth::exchange::parse_token_response(&resp) {
            Ok(tr) => {
                let expires_at = jerk_oauth::expiry::compute_expiry(ctx.now, tr.expires_in, None);
                let new_rt = tr.refresh_token.as_deref().unwrap_or(&grant.refresh_token);
                let scope_val = tr.scope.clone().or(grant.scope.clone());
                let updated = crate::grants::Grant {
                    provider: entry.id.clone(),
                    user: account.clone(),
                    refresh_token: new_rt.to_string(),
                    access_token: Some(tr.access_token.clone()),
                    expires_at,
                    scope: scope_val.clone(),
                };
                if let Err(e) = grants.store(&updated) {
                    let _ = writeln!(
                        io.err,
                        "jerk: {}: cannot store refreshed grant: {e}",
                        entry.id
                    );
                }
                if requested
                    .filter(|req| !jerk_oauth::scopes::scopes_satisfied(scope_val.as_deref(), req))
                    .is_some()
                {
                    return scope_hint(io, &entry.id, requested.unwrap());
                }
                emit_token(
                    &tr.access_token,
                    "oauth",
                    expires_at.map(crate::time::rfc3339).as_deref(),
                    scope_val.as_deref(),
                    &flags,
                    &entry.id,
                    io,
                )
            }
            Err(e) => {
                let _ = writeln!(io.err, "jerk: {}: refresh failed: {e}", entry.id);
                Exit::SignIn
            }
        },
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {}: refresh request failed: {e}", entry.id);
            Exit::SignIn
        }
    }
}

fn emit_token(
    token: &str,
    source: &str,
    expires_at: Option<&str>,
    scope: Option<&str>,
    flags: &crate::dispatch::Flags,
    id: &str,
    io: &mut Io<'_>,
) -> Exit {
    let masked = crate::mask::mask(token);
    let reveal = flags.has("--reveal");
    if flags.has("--json") {
        let mut doc = serde_json::json!({
            "provider": id, "source": source, "masked": masked,
            "expires_at": expires_at, "scopes": scope,
        });
        if reveal {
            doc["token"] = serde_json::json!(token);
        }
        let _ = writeln!(io.out, "{doc}");
        return Exit::Ok;
    }
    let shown = if reveal { token } else { &masked };
    let _ = writeln!(io.out, "{shown}");
    Exit::Ok
}

pub fn create(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match crate::dispatch::parse_flags(args, &["--scope"], &["--no-browser"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} create: {e}", entry.id), io),
    };
    let info = match info_from_entry(entry) {
        Some(i) => i,
        None => return err(io, &entry.id, "missing url"),
    };
    let no_browser = flags.has("--no-browser");
    let scope_override = flags.value("--scope");
    let host = ctx.oauth;
    let _ = writeln!(
        io.err,
        "jerk: {}: discovering OAuth for {}",
        entry.id, info.url
    );
    let (prm, as_meta) = match discover_oauth(&info, host, io, &entry.id) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let resource = prm
        .as_ref()
        .and_then(|p| jerk_oauth::discovery::select_resource(&info.url, p).ok());
    let scope = scope_override
        .map(String::from)
        .or(info.scope.clone())
        .or_else(|| {
            prm.as_ref()
                .map(|p| p.scopes_supported.join(" "))
                .filter(|s| !s.is_empty())
        });
    let (client_id, client_secret) =
        resolve_client(&info, &as_meta, host, &scope, io, &entry.id, ctx.env);
    if client_id.is_empty() {
        return Exit::Usage;
    }
    let mut r32 = [0u8; 32];
    let mut r16 = [0u8; 16];
    (ctx.random)(&mut r32);
    (ctx.random)(&mut r16);
    let verifier = jerk_oauth::pkce::verifier(&r32);
    let challenge = jerk_oauth::pkce::challenge(&verifier);
    let state = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(r16)
    };
    let configured_port = compute_redirect_port(&info);
    let listener = match std::net::TcpListener::bind(format!("127.0.0.1:{}", configured_port)) {
        Ok(l) => l,
        Err(e) => return err(io, &entry.id, &format!("cannot bind loopback: {e}")),
    };
    let _ = listener.set_nonblocking(true);
    let actual_port = listener
        .local_addr()
        .map(|a| a.port())
        .unwrap_or(configured_port);
    let redirect_uri = if let Some(ref cu) = info.callback_url {
        cu.clone()
    } else {
        format!("http://127.0.0.1:{actual_port}/callback")
    };
    let authorize_url = build_authorize_url(
        &as_meta.authorization_endpoint,
        &client_id,
        &redirect_uri,
        &challenge,
        &state,
        scope.as_deref(),
        resource.as_deref(),
    );
    let authorize_url = match authorize_url {
        Ok(u) => u,
        Err(e) => return err(io, &entry.id, &e),
    };
    if no_browser {
        let _ = writeln!(io.err, "Open this URL in your browser:");
        let _ = writeln!(io.err, "{authorize_url}");
        let _ = writeln!(io.err, "Then paste the full redirect URL here:");
        let mut line = String::new();
        if ctx.input.read_line(&mut line).is_err() || line.trim().is_empty() {
            return err(io, &entry.id, "no redirect URL provided");
        }
        return complete_auth(
            entry,
            &info,
            &as_meta,
            &client_id,
            client_secret.as_deref(),
            &verifier,
            &state,
            &redirect_uri,
            resource.as_deref(),
            scope.as_deref(),
            line.trim(),
            host,
            ctx,
            io,
        );
    }
    if let Err(e) = host.browser().open_url(&authorize_url) {
        return err(io, &entry.id, &format!("cannot open browser: {e}"));
    }
    let callback_secs = if cfg!(all(test, jerk_unit)) { 3 } else { 120 };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(callback_secs);
    let callback_url = match wait_for_callback(&listener, deadline) {
        Ok(u) => u,
        Err(e) => return err(io, &entry.id, &e),
    };
    complete_auth(
        entry,
        &info,
        &as_meta,
        &client_id,
        client_secret.as_deref(),
        &verifier,
        &state,
        &redirect_uri,
        resource.as_deref(),
        scope.as_deref(),
        &callback_url,
        host,
        ctx,
        io,
    )
}

#[cfg(all(test, jerk_unit))]
pub fn build_authorize_url_for_test(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    scope: Option<&str>,
    resource: Option<&str>,
) -> Result<String, String> {
    build_authorize_url(
        endpoint,
        client_id,
        redirect_uri,
        challenge,
        state,
        scope,
        resource,
    )
}

fn build_authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    scope: Option<&str>,
    resource: Option<&str>,
) -> Result<String, String> {
    let mut u =
        url::Url::parse(endpoint).map_err(|e| format!("bad authorization_endpoint: {e}"))?;
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("code_challenge", challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("state", state);
        if let Some(s) = scope {
            q.append_pair("scope", s);
        }
        if let Some(r) = resource {
            q.append_pair("resource", r);
        }
    }
    Ok(u.to_string())
}

#[allow(clippy::too_many_arguments)]
fn complete_auth(
    entry: &Entry,
    info: &McpProviderInfo,
    as_meta: &jerk_oauth::discovery::AuthorizationServerMetadata,
    client_id: &str,
    client_secret: Option<&str>,
    verifier: &str,
    state: &str,
    redirect_uri: &str,
    resource: Option<&str>,
    scope: Option<&str>,
    redirect_url: &str,
    host: &dyn crate::oauth::OAuthHost,
    ctx: &Ctx<'_>,
    io: &mut Io<'_>,
) -> Exit {
    let parsed = match url::Url::parse(redirect_url) {
        Ok(u) => u,
        Err(e) => return err(io, &entry.id, &format!("invalid redirect URL: {e}")),
    };
    if let Some(error) = parsed.query_pairs().find(|(k, _)| k == "error") {
        let desc = parsed
            .query_pairs()
            .find(|(k, _)| k == "error_description")
            .map(|(_, v)| v.to_string());
        let _ = writeln!(
            io.err,
            "jerk: {}: authorization error: {}{}",
            entry.id,
            error.1,
            desc.map(|d| format!(": {d}")).unwrap_or_default()
        );
        return Exit::Error;
    }
    let received_state = parsed
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string());
    if received_state.as_deref() != Some(state) {
        return err(io, &entry.id, "state mismatch");
    }
    let received_iss = parsed
        .query_pairs()
        .find(|(k, _)| k == "iss")
        .map(|(_, v)| v.to_string());
    if let Err(e) = jerk_oauth::registration::check_iss(
        received_iss.as_deref(),
        &as_meta.issuer,
        as_meta.authorization_response_iss_parameter_supported,
    ) {
        return err(io, &entry.id, &e);
    }
    let code = match parsed.query_pairs().find(|(k, _)| k == "code") {
        Some((_, v)) => v.to_string(),
        None => return err(io, &entry.id, "no code in redirect"),
    };
    let mut body = jerk_oauth::exchange::build_authorization_code_body(
        &code,
        client_id,
        redirect_uri,
        verifier,
    );
    if let Some(r) = resource {
        body.push(("resource".into(), r.into()));
    }
    if let Some(s) = client_secret {
        body.push(("client_secret".into(), s.into()));
    }
    let form = jerk_oauth::exchange::encode_form_body(&body);
    match host.http_post(&as_meta.token_endpoint, &form) {
        Ok(resp) => match jerk_oauth::exchange::parse_token_response(&resp) {
            Ok(tr) => {
                let expires_at = jerk_oauth::expiry::compute_expiry(ctx.now, tr.expires_in, None);
                let account = grant_account(info);
                let grant = crate::grants::Grant {
                    provider: entry.id.clone(),
                    user: account,
                    refresh_token: tr.refresh_token.unwrap_or_default(),
                    access_token: Some(tr.access_token),
                    expires_at,
                    scope: tr.scope.or_else(|| scope.map(String::from)),
                };
                let grants = host.grants(entry.provider.store.as_deref());
                if let Err(e) = grants.store(&grant) {
                    return err(io, &entry.id, &format!("cannot store grant: {e}"));
                }
                save_client_id(&entry.id, client_id, ctx.env);
                let _ = writeln!(io.out, "{}: signed in", entry.id);
                Exit::Ok
            }
            Err(e) => err(io, &entry.id, &format!("token exchange failed: {e}")),
        },
        Err(e) => err(io, &entry.id, &format!("token request failed: {e}")),
    }
}

pub fn drop_cmd(entry: &Entry, args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let flags = match crate::dispatch::parse_flags(args, &[], &["--yes"], 0) {
        Ok(flags) => flags,
        Err(e) => return usage(&format!("{} drop: {e}", entry.id), io),
    };
    let info = match info_from_entry(entry) {
        Some(i) => i,
        None => return err(io, &entry.id, "missing url"),
    };
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
    let account = grant_account(&info);
    let grant = grants.get(&entry.id, &account);
    if let Some(ref g) = grant {
        let token = g.access_token.as_deref().unwrap_or(&g.refresh_token);
        if !token.is_empty() {
            let client_id = load_client_id(&info, ctx.env, host).unwrap_or_default();
            if let Ok(Some(as_meta)) = try_discover_as(&info, host) {
                if let Some(ref rev_ep) = as_meta.revocation_endpoint {
                    let body = jerk_oauth::registration::build_revocation_body(token, &client_id);
                    let _ = host.http_post(rev_ep, &body);
                }
            }
        }
    }
    if let Err(e) = grants.remove(&entry.id, &account) {
        return err(io, &entry.id, &format!("cannot remove grant: {e}"));
    }
    let _ = writeln!(io.out, "{}: dropped", entry.id);
    Exit::Ok
}

pub fn import_pi(entry: &Entry, _args: &[String], ctx: &mut Ctx<'_>, io: &mut Io<'_>) -> Exit {
    let info = match info_from_entry(entry) {
        Some(i) => i,
        None => return err(io, &entry.id, "missing url"),
    };
    let auth_path = crate::mcp_config::mcp_auth_json_path(ctx.env);
    let text = match std::fs::read_to_string(&auth_path) {
        Ok(t) => t,
        Err(e) => {
            return err(
                io,
                &entry.id,
                &format!("cannot read {}: {e}", auth_path.display()),
            )
        }
    };
    let states: std::collections::BTreeMap<String, serde_json::Value> =
        match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => return err(io, &entry.id, &format!("cannot parse mcp-auth.json: {e}")),
        };
    let ns = info.server_name.replace('-', "_");
    let primary_key = format!("mcp__{ns}|{}", info.url);
    let state = states.get(&primary_key).or_else(|| states.get(&info.url));
    let Some(state) = state else {
        return err(
            io,
            &entry.id,
            &format!("no entry in mcp-auth.json for {}", info.server_name),
        );
    };
    let Some(tokens) = state.get("tokens") else {
        return err(io, &entry.id, "no tokens in mcp-auth.json entry");
    };
    let access_token = tokens
        .get("access_token")
        .and_then(|v| v.as_str())
        .map(String::from);
    let refresh_token = tokens
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let scope = tokens
        .get("scope")
        .and_then(|v| v.as_str())
        .map(String::from);
    let expires_at = state
        .get("tokensExpireAt")
        .and_then(|v| v.as_u64())
        .map(|ms| ms / 1000);
    let account = grant_account(&info);
    let grant = crate::grants::Grant {
        provider: entry.id.clone(),
        user: account,
        refresh_token,
        access_token,
        expires_at,
        scope,
    };
    let host = ctx.oauth;
    let grants = host.grants(entry.provider.store.as_deref());
    if let Err(e) = grants.store(&grant) {
        return err(io, &entry.id, &format!("cannot store grant: {e}"));
    }
    let _ = writeln!(io.out, "{}: imported from pi", entry.id);
    Exit::Ok
}

fn grant_account(info: &McpProviderInfo) -> String {
    format!("{}|{}", info.server_name, info.url)
}

fn sign_in(io: &mut Io<'_>, id: &str) -> Exit {
    let _ = writeln!(io.err, "jerk: {id}: not signed in; run jerk {id} create");
    Exit::SignIn
}

fn scope_hint(io: &mut Io<'_>, id: &str, req: &str) -> Exit {
    let _ = writeln!(
        io.err,
        "jerk: {id}: the token does not cover {req}; run jerk {id} create --scope {req}"
    );
    Exit::SignIn
}

fn err(io: &mut Io<'_>, id: &str, msg: &str) -> Exit {
    let _ = writeln!(io.err, "jerk: {id}: {msg}");
    Exit::Error
}

#[cfg(all(test, jerk_unit))]
pub fn test_discover_oauth(
    info: &McpProviderInfo,
    host: &dyn crate::oauth::OAuthHost,
    io: &mut Io<'_>,
    id: &str,
) -> Result<
    (
        Option<jerk_oauth::discovery::ProtectedResourceMetadata>,
        jerk_oauth::discovery::AuthorizationServerMetadata,
    ),
    Exit,
> {
    discover_oauth(info, host, io, id)
}

#[cfg(all(test, jerk_unit))]
pub fn test_load_client_id(
    info: &McpProviderInfo,
    env: &crate::paths::Env,
    host: &dyn crate::oauth::OAuthHost,
) -> Option<String> {
    load_client_id(info, env, host)
}

#[cfg(all(test, jerk_unit))]
pub fn test_compute_redirect_port(info: &McpProviderInfo) -> u16 {
    compute_redirect_port(info)
}

fn compute_redirect_port(info: &McpProviderInfo) -> u16 {
    if let Some(ref cu) = info.callback_url {
        if let Ok(u) = url::Url::parse(cu) {
            if let Some(p) = u.port() {
                return p;
            }
        }
    }
    info.callback_port.unwrap_or(0)
}

fn client_cache_path(id: &str, env: &crate::paths::Env) -> std::path::PathBuf {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    env.state_dir()
        .join("mcp-clients")
        .join(format!("{safe}.json"))
}

fn save_client_id(id: &str, client_id: &str, env: &crate::paths::Env) {
    let dir = env.state_dir().join("mcp-clients");
    let _ = std::fs::create_dir_all(&dir);
    let data = serde_json::json!({ "client_id": client_id });
    let path = client_cache_path(id, env);
    let _ = std::fs::write(&path, data.to_string());
}

fn load_client_id(
    info: &McpProviderInfo,
    env: &crate::paths::Env,
    host: &dyn crate::oauth::OAuthHost,
) -> Option<String> {
    if let Some(ref cid) = info.client_id {
        return Some(cid.clone());
    }
    let path = client_cache_path(&info.id, env);
    let text = std::fs::read_to_string(path).ok()?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).ok()?;
    let cid = map.get("client_id")?.as_str()?.to_string();
    let _ = host;
    Some(cid)
}

fn discover_oauth(
    info: &McpProviderInfo,
    host: &dyn crate::oauth::OAuthHost,
    io: &mut Io<'_>,
    id: &str,
) -> Result<
    (
        Option<jerk_oauth::discovery::ProtectedResourceMetadata>,
        jerk_oauth::discovery::AuthorizationServerMetadata,
    ),
    Exit,
> {
    if let Some(ref amu) = info.auth_server_metadata_url {
        let as_meta = fetch_as_metadata(amu, host, io, id)?;
        return Ok((None, as_meta));
    }
    let server_url = url::Url::parse(&info.url).map_err(|e| {
        let _ = writeln!(io.err, "jerk: {id}: invalid server URL: {e}");
        Exit::Error
    })?;
    let prm = discover_prm(&server_url, host, io, id);
    let as_url = prm
        .as_ref()
        .and_then(|p| p.authorization_servers.first().cloned())
        .unwrap_or_else(|| {
            let mut u = server_url.clone();
            u.set_path("/");
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        });
    let as_meta = discover_as(&as_url, host, io, id)?;
    Ok((prm, as_meta))
}

fn discover_prm(
    server_url: &url::Url,
    host: &dyn crate::oauth::OAuthHost,
    io: &mut Io<'_>,
    id: &str,
) -> Option<jerk_oauth::discovery::ProtectedResourceMetadata> {
    if let Some(prm) = probe_server(server_url, host) {
        return Some(prm);
    }
    let urls = jerk_oauth::discovery::well_known_prm_urls(server_url);
    for url in urls {
        let parsed = host
            .http_get(url.as_str())
            .ok()
            .and_then(|body| jerk_oauth::discovery::parse_protected_resource_metadata(&body).ok());
        if let Some(prm) = parsed {
            return Some(prm);
        }
    }
    let _ = writeln!(
        io.err,
        "jerk: {id}: no protected resource metadata found; using server origin as AS"
    );
    None
}

fn probe_server(
    server_url: &url::Url,
    host: &dyn crate::oauth::OAuthHost,
) -> Option<jerk_oauth::discovery::ProtectedResourceMetadata> {
    let methods: &[(&str, &str)] = &[
        ("server/discover", "2026-07-28"),
        ("initialize", "2025-03-26"),
    ];
    for (method, version) in methods {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": {
                "protocolVersion": version,
                "capabilities": {},
                "clientInfo": { "name": "jerk", "version": "0.0.0" }
            },
            "id": 1
        });
        let result = host.mcp_probe(server_url.as_str(), &body.to_string());
        if result.status == 401 {
            let prm = result.www_authenticate.as_deref().and_then(|header| {
                let challenge = jerk_oauth::www_authenticate::parse_www_authenticate(header);
                let rm_url = challenge.resource_metadata.as_deref()?;
                let body = host.http_get(rm_url).ok()?;
                jerk_oauth::discovery::parse_protected_resource_metadata(&body).ok()
            });
            if prm.is_some() {
                return prm;
            }
        }
    }
    None
}

fn discover_as(
    as_url: &str,
    host: &dyn crate::oauth::OAuthHost,
    io: &mut Io<'_>,
    id: &str,
) -> Result<jerk_oauth::discovery::AuthorizationServerMetadata, Exit> {
    let urls = jerk_oauth::discovery::as_discovery_urls(as_url);
    for (url, _kind) in &urls {
        match host.http_get(url.as_str()) {
            Ok(body) => match jerk_oauth::discovery::parse_authorization_server_metadata(&body) {
                Ok(meta) => {
                    if let Err(e) = jerk_oauth::discovery::validate_issuer(&meta, as_url) {
                        let _ = writeln!(io.err, "jerk: {id}: {e}");
                        continue;
                    }
                    return Ok(meta);
                }
                Err(e) => {
                    let _ = writeln!(io.err, "jerk: {id}: AS metadata at {url}: {e}");
                }
            },
            Err(_) => continue,
        }
    }
    let _ = writeln!(
        io.err,
        "jerk: {id}: cannot discover authorization server metadata"
    );
    Err(Exit::Error)
}

fn fetch_as_metadata(
    url: &str,
    host: &dyn crate::oauth::OAuthHost,
    io: &mut Io<'_>,
    id: &str,
) -> Result<jerk_oauth::discovery::AuthorizationServerMetadata, Exit> {
    match host.http_get(url) {
        Ok(body) => {
            jerk_oauth::discovery::parse_authorization_server_metadata(&body).map_err(|e| {
                let _ = writeln!(io.err, "jerk: {id}: AS metadata at {url}: {e}");
                Exit::Error
            })
        }
        Err(e) => {
            let _ = writeln!(io.err, "jerk: {id}: cannot fetch {url}: {e}");
            Err(Exit::Error)
        }
    }
}

fn try_discover_as(
    info: &McpProviderInfo,
    host: &dyn crate::oauth::OAuthHost,
) -> Result<Option<jerk_oauth::discovery::AuthorizationServerMetadata>, ()> {
    let as_url = info
        .auth_server_metadata_url
        .clone()
        .or_else(|| {
            let u = url::Url::parse(&info.url).ok()?;
            let mut root = u.clone();
            root.set_path("/");
            root.set_query(None);
            root.set_fragment(None);
            Some(root.to_string())
        })
        .ok_or(())?;
    let urls = jerk_oauth::discovery::as_discovery_urls(&as_url);
    for (url, _) in &urls {
        let parsed = host.http_get(url.as_str()).ok().and_then(|body| {
            jerk_oauth::discovery::parse_authorization_server_metadata(&body).ok()
        });
        if let Some(meta) = parsed {
            return Ok(Some(meta));
        }
    }
    Ok(None)
}

fn resolve_client(
    info: &McpProviderInfo,
    as_meta: &jerk_oauth::discovery::AuthorizationServerMetadata,
    host: &dyn crate::oauth::OAuthHost,
    scope: &Option<String>,
    io: &mut Io<'_>,
    id: &str,
    env: &crate::paths::Env,
) -> (String, Option<String>) {
    let iss_supported = as_meta.authorization_response_iss_parameter_supported;
    if info.client_registration.as_deref() == Some("cimd")
        || (as_meta.client_id_metadata_document_supported
            && iss_supported
            && info.client_id.is_none()
            && info.client_registration.is_none())
    {
        let cimd_url = "https://mcpcat.sliccy.ai/oauth/client.json";
        return (cimd_url.to_string(), None);
    }
    if as_meta.registration_endpoint.is_some() && info.client_id.is_none() {
        let reg_ep = as_meta.registration_endpoint.as_deref().unwrap();
        let client_name = info.client_name.as_deref().unwrap_or("mcpcat");
        let redirect_uri = if let Some(ref cu) = info.callback_url {
            cu.clone()
        } else {
            let port = info.callback_port.unwrap_or(0);
            format!("http://127.0.0.1:{port}/callback")
        };
        let body =
            jerk_oauth::registration::build_dcr_body(&redirect_uri, client_name, scope.as_deref());
        match host.http_post_json(reg_ep, &body) {
            Ok(resp) => match jerk_oauth::registration::parse_dcr_response(&resp) {
                Ok(ci) => {
                    let _ = writeln!(io.err, "jerk: {id}: registered client {}", ci.client_id);
                    save_client_id(id, &ci.client_id, env);
                    if let Some(ref secret) = ci.client_secret {
                        let grants = host.grants(None);
                        let cg = crate::grants::Grant {
                            provider: format!("{id}:client"),
                            user: "dcr".into(),
                            refresh_token: String::new(),
                            access_token: Some(secret.clone()),
                            expires_at: None,
                            scope: None,
                        };
                        let _ = grants.store(&cg);
                    }
                    return (ci.client_id, ci.client_secret);
                }
                Err(e) => {
                    let _ = writeln!(io.err, "jerk: {id}: DCR failed: {e}");
                }
            },
            Err(e) => {
                let _ = writeln!(io.err, "jerk: {id}: DCR request failed: {e}");
            }
        }
    }
    if let Some(ref cid) = info.client_id {
        return (cid.clone(), info.client_secret.clone());
    }
    let _ = writeln!(
        io.err,
        "jerk: {id}: no client registration method available"
    );
    (String::new(), None)
}

fn wait_for_callback(
    listener: &std::net::TcpListener,
    deadline: std::time::Instant,
) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write as IoWrite};
    let html = "<!DOCTYPE html><html><body><p>Signed in, you can close this tab.</p></body></html>";
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("timed out waiting for callback".into());
        }
        let stream = match listener.accept() {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
        };
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
        let mut reader = BufReader::new(&stream);
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 || parts[0] != "GET" {
            continue;
        }
        let full_url = format!("http://127.0.0.1{}", parts[1]);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(), html
        );
        let _ = (&stream).write_all(response.as_bytes());
        return Ok(full_url);
    }
}
