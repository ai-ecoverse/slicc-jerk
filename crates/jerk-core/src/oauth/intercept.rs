use crate::exit::Exit;
use crate::oauth::{self, OAuthError, Token};
use cdp_client::cdp::Cdp;
use cdp_client::fetch::{self, Decision, Fulfill, Interception, Overrides, Pattern};
use jerk_oauth::capture;
use jerk_oauth::config::{CaptureAt, CaptureFrom, Flow, OAuthConfig};
use jerk_oauth::pkce;
use jerk_oauth::relay;
use jerk_oauth::rewrite;
use jerk_oauth::template;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;
use std::time::{Duration, Instant};

struct Captured {
    value: String,
    fields: BTreeMap<String, String>,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    cdp: &mut Cdp,
    config: &OAuthConfig,
    scope: Option<&str>,
    now_secs: u64,
    random_32: &[u8; 32],
    random_16: &[u8; 16],
    http_post: &oauth::HttpPost<'_>,
    err: &mut dyn Write,
) -> Result<Token, OAuthError> {
    let timeout_ms = jerk_oauth::config::timeout_ms(config);

    let (verifier, challenge) = if config.flow == Flow::Pkce {
        let v = pkce::verifier(random_32);
        let c = pkce::challenge(&v);
        (Some(v), Some(c))
    } else {
        (None, None)
    };

    let state = if let Some(r) = &config.relay {
        relay::build_state(r, random_16)
    } else {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(random_16)
    };

    let nonce = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        URL_SAFE_NO_PAD.encode(&random_16[..8])
    };

    let actual_scope = scope.or(config.scope.as_deref()).unwrap_or("");

    let mut vars: Vec<(&str, &str)> = vec![
        ("client_id", &config.client_id),
        ("redirect_uri", &config.redirect_uri),
        ("scope", actual_scope),
        ("state", &state),
        ("nonce", &nonce),
    ];
    if let Some(c) = &challenge {
        vars.push(("code_challenge", c));
    }
    let authorize_url = template::render(&config.authorize_url, &vars);
    let mut config = config.clone();
    if let Some(url) = config.capture.url.clone() {
        let secret = config.client_secret.clone();
        config.capture.url = Some(rendered_capture_url(&url, &vars, secret.as_deref())?);
    }

    let browser_context = if config.isolate == Some(true) {
        let result = cdp
            .call("Target.createBrowserContext", json!({}), None)
            .map_err(map_cdp_error)?;
        result["browserContextId"].as_str().map(String::from)
    } else {
        None
    };

    let target_params = match &browser_context {
        Some(ctx_id) => json!({ "url": "about:blank", "browserContextId": ctx_id }),
        None => json!({ "url": "about:blank" }),
    };
    let result = cdp
        .call("Target.createTarget", target_params, None)
        .map_err(map_cdp_error)?;
    let target_id = result["targetId"]
        .as_str()
        .ok_or_else(|| OAuthError {
            exit: Exit::Error,
            message: "Target.createTarget returned no targetId".into(),
        })?
        .to_string();

    let outcome = run_in_target(
        cdp,
        &target_id,
        &config,
        &authorize_url,
        &state,
        random_16,
        timeout_ms,
        err,
    );

    let _ = cdp.call("Target.closeTarget", json!({ "targetId": target_id }), None);
    if let Some(ctx_id) = &browser_context {
        let _ = cdp.call(
            "Target.disposeBrowserContext",
            json!({ "browserContextId": ctx_id }),
            None,
        );
    }

    let captured = outcome?;

    if config.capture.at == CaptureAt::Network {
        if matches!(config.capture.from, CaptureFrom::Header | CaptureFrom::Body) {
            return Ok(Token {
                access_token: captured.value,
                refresh_token: None,
                expires_at: None,
                scope: None,
                token_type: None,
                fields: captured.fields,
            });
        }
        jerk_oauth::config::require_https_or_loopback(&captured.value).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: format!("captured network URL: {e}"),
        })?;
    }

    let redirect_target = match &config.relay {
        Some(r) => Some(relay::relay_redirect_target(r)),
        None => Some(config.redirect_uri.clone()),
    };
    let expected_state = if jerk_oauth::config::expects_state(&config.authorize_url) {
        Some(state.as_str())
    } else {
        None
    };
    let result = capture::parse_redirect(
        &captured.value,
        &config.capture.from,
        &config.capture.param,
        expected_state,
        if config.capture.at == CaptureAt::Network {
            None
        } else {
            redirect_target.as_deref()
        },
        None,
    )
    .map_err(|e| OAuthError {
        exit: Exit::Error,
        message: e.to_string(),
    })?;

    if config.relay.is_some() {
        if let Some(received_nonce) = result.params.get("nonce") {
            let expected = relay::nonce_from_bytes(random_16);
            relay::verify_nonce(&expected, received_nonce).map_err(|e| OAuthError {
                exit: Exit::Error,
                message: e.to_string(),
            })?;
        }
    }

    oauth::token_from_capture(
        &config,
        &result,
        verifier.as_deref(),
        scope,
        now_secs,
        http_post,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_in_target(
    cdp: &mut Cdp,
    target_id: &str,
    config: &OAuthConfig,
    authorize_url: &str,
    state: &str,
    nonce_bytes: &[u8; 16],
    timeout_ms: u64,
    err: &mut dyn Write,
) -> Result<Captured, OAuthError> {
    let mut patterns = build_patterns(config);

    if patterns.is_empty() && config.capture.at != CaptureAt::Navigation {
        return Err(OAuthError {
            exit: Exit::Usage,
            message: "no fetch patterns to intercept".into(),
        });
    }

    if config.capture.at == CaptureAt::Navigation && patterns.is_empty() {
        patterns.push(Pattern::request("__jerk_placeholder_never_matches__"));
    }

    let mut interception =
        Interception::attach(cdp, target_id, &patterns).map_err(map_cdp_error)?;

    let session = interception.session_id().to_string();
    interception
        .call("Page.enable", json!({}), Some(&session))
        .map_err(map_cdp_error)?;

    let _ = writeln!(err, "jerk: sign-in opened in browser");

    interception
        .notify(
            "Page.navigate",
            json!({ "url": authorize_url }),
            Some(&session),
        )
        .map_err(map_cdp_error)?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    let effective_redirect = match &config.relay {
        Some(r) => relay::relay_redirect_target(r),
        None => config.redirect_uri.clone(),
    };
    let redirect_base = redirect_base_url(&effective_redirect);

    match config.capture.at {
        CaptureAt::Navigation => capture_navigation(
            &mut interception,
            &session,
            &redirect_base,
            config,
            deadline,
        ),
        CaptureAt::Network => capture_fetch(
            &mut interception,
            config,
            &redirect_base,
            state,
            nonce_bytes,
            deadline,
            true,
        ),
        _ => capture_fetch(
            &mut interception,
            config,
            &redirect_base,
            state,
            nonce_bytes,
            deadline,
            false,
        ),
    }
}

fn capture_navigation(
    interception: &mut Interception<'_>,
    _session: &str,
    redirect_base: &str,
    config: &OAuthConfig,
    deadline: Instant,
) -> Result<Captured, OAuthError> {
    let redirect_check = redirect_base.to_string();
    loop {
        let remaining = remaining_timeout(deadline)?;

        match interception.next(remaining.min(Duration::from_millis(200))) {
            Ok(Some(paused)) => {
                let decision = rewrite_decision(&paused, config);
                let _ = interception.decide(&paused, &decision);
            }
            Ok(None) => {}
            Err(_) => {}
        }

        let guard = interception.guard().clone();
        let check = redirect_check.clone();
        if let Ok(Some(event)) = interception.wait_event(0, Duration::from_millis(100), |event| {
            fetch::navigation(&guard, event)
                .is_some_and(|nav| nav.url.starts_with(&check) || nav.url.contains(&check))
        }) {
            if let Some(nav) = fetch::navigation(&guard, &event) {
                return Ok(Captured {
                    value: nav.url,
                    fields: BTreeMap::new(),
                });
            }
        }

        if Instant::now() >= deadline {
            return Err(timeout_err());
        }
    }
}

fn capture_fetch(
    interception: &mut Interception<'_>,
    config: &OAuthConfig,
    redirect_base: &str,
    _state: &str,
    _nonce_bytes: &[u8; 16],
    deadline: Instant,
    is_network: bool,
) -> Result<Captured, OAuthError> {
    loop {
        let remaining = remaining_timeout(deadline)?;

        let paused = match interception.next(remaining).map_err(map_cdp_error) {
            Ok(Some(p)) => p,
            Ok(None) => return Err(timeout_err()),
            Err(e) => return Err(e),
        };

        if is_network {
            if let Some(value) = try_capture_network(&paused, config) {
                if let Some(fields) = capture_fields(interception, &paused, config) {
                    let _ = interception.decide(&paused, &Decision::default());
                    return Ok(Captured { value, fields });
                }
            }
        } else if let Some(captured) = try_capture(&paused, config, redirect_base) {
            let fulfill = match config.on_capture.as_ref() {
                Some(jerk_oauth::config::OnCapture::Close) => {
                    Decision::Fail(cdp_client::fetch::ErrorReason::Aborted)
                }
                Some(jerk_oauth::config::OnCapture::Leave) => Decision::default(),
                _ => Decision::Fulfill(Fulfill::signed_in()),
            };
            let _ = interception.decide(&paused, &fulfill);
            return Ok(Captured {
                value: captured,
                fields: BTreeMap::new(),
            });
        }

        let decision = rewrite_decision(&paused, config);
        let _ = interception.decide(&paused, &decision);
    }
}

fn try_capture(
    paused: &cdp_client::fetch::Paused,
    config: &OAuthConfig,
    redirect_base: &str,
) -> Option<String> {
    match config.capture.at {
        CaptureAt::Request => {
            let url = paused.full_url();
            if url.starts_with(redirect_base) {
                Some(url)
            } else {
                None
            }
        }
        CaptureAt::Response => {
            if !paused.is_response() {
                return None;
            }
            let location = paused.location()?;
            if location.starts_with(redirect_base) || location.contains(redirect_base) {
                Some(location)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn try_capture_network(paused: &cdp_client::fetch::Paused, config: &OAuthConfig) -> Option<String> {
    let capture_url = config.capture.url.as_deref()?;
    if !url_matches_pattern(&paused.url, capture_url) {
        return None;
    }
    match config.capture.from {
        CaptureFrom::Header => {
            let header_name = config.capture.header.as_deref()?;
            let value = paused.header(header_name)?;
            Some(value.to_string())
        }
        CaptureFrom::Body => {
            if config.capture.stage == Some(jerk_oauth::config::CaptureStage::Request) {
                if paused.is_response() {
                    return None;
                }
                return body_param(paused.post_data.as_deref()?, &config.capture.param);
            }
            if paused.error_reason.is_some() || !matches!(paused.status, Some(s) if s < 400) {
                return None;
            }
            body_param(paused.post_data.as_deref()?, &config.capture.param)
        }
        CaptureFrom::Query => Some(paused.full_url()),
        CaptureFrom::Url => Some(paused.full_url()),
        _ => None,
    }
}

fn capture_fields(
    interception: &mut Interception<'_>,
    paused: &cdp_client::fetch::Paused,
    config: &OAuthConfig,
) -> Option<BTreeMap<String, String>> {
    if config.capture.fields.is_empty() {
        return Some(BTreeMap::new());
    }
    let mut fields = BTreeMap::new();
    for (name, spec) in &config.capture.fields {
        fields.insert(name.clone(), field_value(interception, paused, spec)?);
    }
    Some(fields)
}

fn field_value(
    interception: &mut Interception<'_>,
    paused: &cdp_client::fetch::Paused,
    spec: &str,
) -> Option<String> {
    let (kind, rest) = spec.split_once(':')?;
    let value = match kind {
        "cookie" => cookie_from_header(paused.header("cookie").unwrap_or(""), rest)
            .or_else(|| cookie_from_network(interception, paused, rest)),
        "header" => paused
            .header(rest)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        "body" => body_param(paused.post_data.as_deref()?, rest),
        "query" => query_param(&paused.full_url(), rest),
        _ => None,
    };
    value.filter(|item| !item.is_empty())
}

pub(crate) fn cookie_from_header(header: &str, name: &str) -> Option<String> {
    header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name && !value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

fn cookie_from_network(
    interception: &mut Interception<'_>,
    paused: &cdp_client::fetch::Paused,
    name: &str,
) -> Option<String> {
    let session = interception.session_id().to_string();
    let result = interception
        .call(
            "Network.getCookies",
            json!({ "urls": [paused.url] }),
            Some(&session),
        )
        .ok()?;
    let cookies = result.get("cookies")?.as_array()?;
    cookies.iter().find_map(|cookie| {
        let key = cookie.get("name").and_then(|value| value.as_str())?;
        let value = cookie.get("value").and_then(|item| item.as_str())?;
        (key == name && !value.is_empty()).then(|| value.to_string())
    })
}

fn query_param(url: &str, name: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    parsed
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
}

fn body_param(body: &[u8], param: &str) -> Option<String> {
    let value = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(json) => json.get(param)?.as_str()?.to_string(),
        Err(_) => url::form_urlencoded::parse(body)
            .find(|(name, _)| name == param)?
            .1
            .into_owned(),
    };
    (!value.is_empty()).then_some(value)
}

fn rewrite_decision(paused: &cdp_client::fetch::Paused, config: &OAuthConfig) -> Decision {
    if paused.is_response() {
        return Decision::default();
    }
    let rewritten = rewrite::apply_rewrites(&paused.url, &config.rewrite);
    if rewritten != paused.url {
        Decision::Continue(Overrides {
            url: Some(rewritten),
            ..Overrides::default()
        })
    } else {
        Decision::default()
    }
}

pub(crate) fn rendered_capture_url(
    url: &str,
    vars: &[(&str, &str)],
    client_secret: Option<&str>,
) -> Result<String, OAuthError> {
    let mut pairs = vars.to_vec();
    if let Some(secret) = client_secret {
        pairs.push(("client_secret", secret));
    }
    let rendered = template::render(url, &pairs);
    if let Some(name) = leftover_placeholder(&rendered) {
        let message = if name.is_empty() {
            "capture.url has an unclosed placeholder".to_string()
        } else {
            format!("capture.url still has placeholder {{{name}}}")
        };
        return Err(OAuthError {
            exit: Exit::Usage,
            message,
        });
    }
    Ok(rendered)
}

fn leftover_placeholder(template: &str) -> Option<String> {
    let start = template.find('{')?;
    let rest = &template[start + 1..];
    match rest.find('}') {
        None => Some(String::new()),
        Some(end) => Some(rest[..end].to_string()),
    }
}

fn build_patterns(config: &OAuthConfig) -> Vec<Pattern> {
    let mut patterns = Vec::new();

    for rule in &config.rewrite {
        patterns.push(Pattern::request(format!("*{}*", rule.match_pattern)));
    }

    match config.capture.at {
        CaptureAt::Request => {
            let effective = match &config.relay {
                Some(r) => relay::relay_redirect_target(r),
                None => config.redirect_uri.clone(),
            };
            let pattern = redirect_pattern(&effective);
            patterns.push(Pattern::request(pattern));
        }
        CaptureAt::Response => {
            let pattern = authorize_host_pattern(&config.authorize_url);
            patterns.push(Pattern::response(pattern));
        }
        CaptureAt::Network => {
            if let Some(url) = &config.capture.url {
                let on_request = config.capture.from != CaptureFrom::Body
                    || config.capture.stage == Some(jerk_oauth::config::CaptureStage::Request);
                patterns.push(if on_request {
                    Pattern::request(url.clone())
                } else {
                    Pattern::response(url.clone())
                });
            }
        }
        CaptureAt::Navigation | CaptureAt::Callback => {}
    }

    patterns
}

fn redirect_pattern(redirect_uri: &str) -> String {
    match redirect_uri.find('?') {
        Some(i) => format!("{}*", &redirect_uri[..i]),
        None => format!("{redirect_uri}*"),
    }
}

fn authorize_host_pattern(authorize_url: &str) -> String {
    match url::Url::parse(authorize_url) {
        Ok(parsed) => format!(
            "{}://{}/*",
            parsed.scheme(),
            parsed.host_str().unwrap_or("*")
        ),
        Err(_) => "*".to_string(),
    }
}

fn redirect_base_url(redirect_uri: &str) -> String {
    match redirect_uri.find('?') {
        Some(i) => redirect_uri[..i].to_string(),
        None => redirect_uri.to_string(),
    }
}

fn url_matches_pattern(url: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return url == pattern;
    }
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match url[pos..].find(part) {
            Some(found) => {
                if i == 0 && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    if !parts.last().unwrap_or(&"").is_empty() {
        return pos == url.len();
    }
    true
}

fn remaining_timeout(deadline: Instant) -> Result<Duration, OAuthError> {
    deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(timeout_err)
}

fn timeout_err() -> OAuthError {
    OAuthError {
        exit: Exit::Error,
        message: "timed out waiting for sign-in".into(),
    }
}

pub(crate) fn is_blocked_error(message: &str) -> bool {
    message.contains("BlockedByClient") || message.starts_with("slicc: refused:")
}

pub(crate) fn map_cdp_error(message: String) -> OAuthError {
    if is_blocked_error(&message) {
        OAuthError {
            exit: Exit::Usage,
            message: "sign-in blocked by browser (protected origin)".into(),
        }
    } else {
        OAuthError {
            exit: Exit::Error,
            message,
        }
    }
}
