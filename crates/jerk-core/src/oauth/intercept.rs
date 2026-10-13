use crate::exit::Exit;
use crate::oauth::{OAuthError, Token};
use cdp_client::cdp::Cdp;
use cdp_client::fetch::{self, Decision, Fulfill, Interception, Overrides, Pattern};
use jerk_oauth::capture;
use jerk_oauth::config::{CaptureAt, CaptureFrom, Flow, OAuthConfig};
use jerk_oauth::exchange;
use jerk_oauth::expiry;
use jerk_oauth::pkce;
use jerk_oauth::relay;
use jerk_oauth::rewrite;
use jerk_oauth::template;
use serde_json::json;
use std::io::Write;
use std::time::{Duration, Instant};

#[allow(clippy::too_many_arguments)]
pub fn run(
    cdp: &mut Cdp,
    config: &OAuthConfig,
    scope: Option<&str>,
    now_secs: u64,
    random_32: &[u8; 32],
    random_16: &[u8; 16],
    http_post: &dyn Fn(&str, &str) -> Result<String, String>,
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
        config,
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
        if config.capture.from == CaptureFrom::Header {
            return Ok(Token {
                access_token: captured,
                refresh_token: None,
                expires_at: None,
                scope: None,
                token_type: None,
            });
        }
        jerk_oauth::config::require_https_or_loopback(&captured).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: format!("captured network URL: {e}"),
        })?;
    }

    let redirect_target = match &config.relay {
        Some(r) => Some(relay::relay_redirect_target(r)),
        None => Some(config.redirect_uri.clone()),
    };
    let result = capture::parse_redirect(
        &captured,
        &config.capture.from,
        &config.capture.param,
        Some(&state),
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

    if config.flow == Flow::Pkce {
        let verifier = verifier.as_deref().unwrap();
        let token_url = config.token_url.as_deref().ok_or_else(|| OAuthError {
            exit: Exit::Usage,
            message: "token_url required for pkce flow".into(),
        })?;
        let body = exchange::build_authorization_code_body(
            &result.value,
            &config.client_id,
            &config.redirect_uri,
            verifier,
        );
        let form_body = exchange::encode_form_body(&body);
        let response = http_post(token_url, &form_body).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e,
        })?;
        let token_response = exchange::parse_token_response(&response).map_err(|e| OAuthError {
            exit: Exit::Error,
            message: e.to_string(),
        })?;
        let expires_at = expiry::compute_expiry(now_secs, token_response.expires_in, None);
        Ok(Token {
            access_token: token_response.access_token.clone(),
            refresh_token: token_response.refresh_token.clone(),
            expires_at,
            scope: token_response.scope.clone(),
            token_type: token_response.token_type.clone(),
        })
    } else {
        let expires_in = result
            .params
            .get(config.expires_param.as_deref().unwrap_or("expires_in"))
            .and_then(|v| v.parse::<u64>().ok());
        let expires_at = expiry::compute_expiry(now_secs, expires_in, None);
        Ok(Token {
            access_token: result.value,
            refresh_token: None,
            expires_at,
            scope: result.params.get("scope").cloned(),
            token_type: result.params.get("token_type").cloned(),
        })
    }
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
) -> Result<String, OAuthError> {
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
) -> Result<String, OAuthError> {
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
                return Ok(nav.url);
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
) -> Result<String, OAuthError> {
    loop {
        let remaining = remaining_timeout(deadline)?;

        let paused = match interception.next(remaining).map_err(map_cdp_error) {
            Ok(Some(p)) => p,
            Ok(None) => return Err(timeout_err()),
            Err(e) => return Err(e),
        };

        if is_network {
            if let Some(captured) = try_capture_network(&paused, config) {
                let _ = interception.decide(&paused, &Decision::default());
                return Ok(captured);
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
            return Ok(captured);
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
        CaptureFrom::Query => Some(paused.full_url()),
        CaptureFrom::Url => Some(paused.full_url()),
        _ => None,
    }
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
                patterns.push(Pattern::request(url.clone()));
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
