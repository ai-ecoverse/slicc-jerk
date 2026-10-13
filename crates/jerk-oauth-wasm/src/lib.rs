use jerk_oauth::backoff;
use jerk_oauth::capture;
use jerk_oauth::config::{self, Flow, OAuthConfig};
use jerk_oauth::exchange;
use jerk_oauth::expiry;
use jerk_oauth::pkce;
use jerk_oauth::relay;
use jerk_oauth::rewrite;
use jerk_oauth::scopes;
use jerk_oauth::template;
use wasm_bindgen::prelude::*;
use zeroize::Zeroize;

fn make_error(code: &str, message: &str) -> JsValue {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"kind".into(), &"error".into()).ok();
    js_sys::Reflect::set(&obj, &"code".into(), &code.into()).ok();
    js_sys::Reflect::set(&obj, &"message".into(), &message.into()).ok();
    obj.into()
}

fn make_idp_error(error: &str, description: Option<&String>) -> JsValue {
    let message = match description {
        Some(d) => format!("{error}: {d}"),
        None => error.to_string(),
    };
    let obj = make_error("idp_error", &message);
    js_sys::Reflect::set(&obj, &"error".into(), &error.into()).ok();
    if let Some(d) = description {
        js_sys::Reflect::set(&obj, &"description".into(), &d.as_str().into()).ok();
    }
    obj
}

fn extract_query_param(url_str: &str, name: &str) -> Option<String> {
    let query = url_str.find('?').map(|i| {
        let after = &url_str[i + 1..];
        match after.find('#') {
            Some(j) => &after[..j],
            None => after,
        }
    })?;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == name {
                return Some(percent_decode(v));
            }
        }
    }
    None
}

fn percent_decode(input: &str) -> String {
    let mut out = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                out.push(hi << 4 | lo);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn sanitize_serde_error(msg: &str) -> String {
    if msg.contains("unknown variant") {
        return "invalid config".into();
    }
    for (needle, kind) in [
        ("unknown field `", "unknown"),
        ("missing field `", "missing"),
    ] {
        if let Some(raw) = extract_between(msg, needle, "`") {
            return match jerk_oauth::config::safe_field_name(raw) {
                Some(name) => format!("invalid config: field {name}"),
                None => format!("invalid config: {kind} field"),
            };
        }
    }
    "invalid config".into()
}

fn extract_between<'a>(haystack: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find(suffix)?;
    Some(&rest[..end])
}

fn sanitize_toml_error(msg: &str) -> String {
    for line in msg.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("TOML parse error at line ") {
            if let Some(rest) = trimmed.strip_prefix("TOML parse error at line ") {
                if let Some(comma) = rest.find(',') {
                    if let Ok(n) = rest[..comma].parse::<u64>() {
                        return format!("invalid config: line {n}");
                    }
                }
            }
        }
    }
    "invalid config".into()
}

fn make_parse_error(msg: &str) -> JsValue {
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"ok".into(), &false.into()).ok();
    js_sys::Reflect::set(
        &obj,
        &"errors".into(),
        &serde_json::to_string(&[msg]).unwrap_or_default().into(),
    )
    .ok();
    obj.into()
}

fn parse_oauth_config(input: &str) -> Result<OAuthConfig, JsValue> {
    let trimmed = input.trim_start();
    if trimmed.starts_with('{') {
        serde_json::from_str::<OAuthConfig>(trimmed)
            .map_err(|e| make_parse_error(&sanitize_serde_error(&e.to_string())))
    } else {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            oauth: OAuthConfig,
        }
        let wrapper: Wrapper = toml_parse(trimmed)?;
        Ok(wrapper.oauth)
    }
}

fn validate_config(config: OAuthConfig) -> Result<config::ValidConfig, JsValue> {
    config::validated(config).map_err(|errors| {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        make_error("config_invalid", &msgs.join("; "))
    })
}

fn toml_parse<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, JsValue> {
    let value: serde_json::Value = toml_to_json(input)
        .map_err(|e| -> JsValue { make_parse_error(&sanitize_toml_error(&e)) })?;
    serde_json::from_value(value)
        .map_err(|e| make_parse_error(&sanitize_serde_error(&e.to_string())))
}

fn toml_to_json(input: &str) -> Result<serde_json::Value, String> {
    let table: toml::Table = input.parse().map_err(|e: toml::de::Error| e.to_string())?;
    toml_value_to_json(toml::Value::Table(table))
}

fn toml_value_to_json(v: toml::Value) -> Result<serde_json::Value, String> {
    match v {
        toml::Value::String(s) => Ok(serde_json::Value::String(s)),
        toml::Value::Integer(i) => Ok(serde_json::json!(i)),
        toml::Value::Float(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .ok_or_else(|| "invalid float".to_string()),
        toml::Value::Boolean(b) => Ok(serde_json::Value::Bool(b)),
        toml::Value::Datetime(d) => Ok(serde_json::Value::String(d.to_string())),
        toml::Value::Array(arr) => {
            let items: Result<Vec<_>, _> = arr.into_iter().map(toml_value_to_json).collect();
            Ok(serde_json::Value::Array(items?))
        }
        toml::Value::Table(tbl) => {
            let mut map = serde_json::Map::new();
            for (k, val) in tbl {
                map.insert(k, toml_value_to_json(val)?);
            }
            Ok(serde_json::Value::Object(map))
        }
    }
}

#[wasm_bindgen(js_name = parseConfig)]
pub fn parse_config(input: &str) -> JsValue {
    match parse_oauth_config(input) {
        Ok(cfg) => {
            let errors = config::validate(&cfg);
            if errors.is_empty() {
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"ok".into(), &true.into()).ok();
                js_sys::Reflect::set(
                    &obj,
                    &"config".into(),
                    &serde_json::to_string(&cfg).unwrap_or_default().into(),
                )
                .ok();
                obj.into()
            } else {
                let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
                let obj = js_sys::Object::new();
                js_sys::Reflect::set(&obj, &"ok".into(), &false.into()).ok();
                js_sys::Reflect::set(
                    &obj,
                    &"errors".into(),
                    &serde_json::to_string(&msgs).unwrap_or_default().into(),
                )
                .ok();
                obj.into()
            }
        }
        Err(e) => e,
    }
}

#[wasm_bindgen(js_name = beginSignIn)]
pub fn begin_sign_in(
    config_json: &str,
    scope: &str,
    now_secs: f64,
    random32: &[u8],
    random16: &[u8],
) -> Result<JsValue, JsValue> {
    if random32.len() != 32 {
        return Err(make_error("invalid_input", "random32 must be 32 bytes"));
    }
    if random16.len() != 16 {
        return Err(make_error("invalid_input", "random16 must be 16 bytes"));
    }

    let r32: [u8; 32] = random32.try_into().unwrap();
    let r16: [u8; 16] = random16.try_into().unwrap();

    config::validate_entropy(&r32, &r16).map_err(|msg| make_error("bad_entropy", msg))?;

    let raw_cfg = parse_oauth_config(config_json)?;
    let valid = validate_config(raw_cfg)?;
    let cfg = valid.inner();

    let mut verifier_str = pkce::verifier(&r32);
    let challenge = pkce::challenge(&verifier_str);
    let nonce = relay::nonce_from_bytes(&r16);

    let effective_scope = if scope.is_empty() {
        cfg.scope.as_deref().unwrap_or("")
    } else {
        scope
    };

    let (state_value, expected_nonce) = if let Some(ref rel) = cfg.relay {
        let st = relay::build_state(rel, &r16);
        let n = nonce.clone();
        (st, Some(n))
    } else {
        (nonce.clone(), None)
    };

    let vars = [
        ("client_id", cfg.client_id.as_str()),
        ("redirect_uri", cfg.redirect_uri.as_str()),
        ("scope", effective_scope),
        ("state", &state_value),
        ("code_challenge", &challenge),
        ("nonce", &nonce),
    ];
    let raw_url = template::render(&cfg.authorize_url, &vars);
    let authorize_url = rewrite::apply_rewrites(&raw_url, &cfg.rewrite);

    let pending = serde_json::json!({
        "verifier": verifier_str,
        "state": state_value,
        "expected_nonce": expected_nonce,
        "flow": cfg.flow,
        "now_secs": now_secs as u64,
    });

    verifier_str.zeroize();

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"authorizeUrl".into(), &authorize_url.into()).ok();
    js_sys::Reflect::set(&obj, &"state".into(), &state_value.into()).ok();
    if let Some(n) = &expected_nonce {
        js_sys::Reflect::set(&obj, &"expectedNonce".into(), &n.clone().into()).ok();
    }
    js_sys::Reflect::set(
        &obj,
        &"pending".into(),
        &serde_json::to_string(&pending).unwrap_or_default().into(),
    )
    .ok();

    Ok(obj.into())
}

#[wasm_bindgen(js_name = completeSignIn)]
pub fn complete_sign_in(
    config_json: &str,
    pending_json: &str,
    captured_url: &str,
) -> Result<JsValue, JsValue> {
    let raw_cfg = parse_oauth_config(config_json)?;
    let valid = validate_config(raw_cfg)?;
    let cfg = valid.inner();

    #[derive(serde::Deserialize)]
    struct Pending {
        verifier: String,
        state: String,
        #[serde(default)]
        expected_nonce: Option<String>,
        flow: Flow,
        now_secs: u64,
    }

    let pending: Pending = serde_json::from_str(pending_json)
        .map_err(|_| make_error("invalid_pending", "invalid pending state"))?;

    let redirect_check = match &cfg.relay {
        Some(r) => relay::relay_redirect_target(r),
        None => cfg.redirect_uri.clone(),
    };

    let result = capture::parse_redirect(
        captured_url,
        &cfg.capture.from,
        &cfg.capture.param,
        Some(&pending.state),
        Some(&redirect_check),
        None,
    )
    .map_err(|e| -> JsValue {
        let (code, msg) = match &e {
            capture::CaptureError::MissingParam(name) => (
                "missing_param".to_string(),
                format!("missing parameter: {name}"),
            ),
            capture::CaptureError::StateMismatch => {
                ("state_mismatch".into(), "state mismatch".into())
            }
            capture::CaptureError::RedirectMismatch => {
                ("redirect_mismatch".into(), "redirect URI mismatch".into())
            }
            capture::CaptureError::DuplicateParam(name) => (
                "duplicate_param".into(),
                format!("duplicate parameter: {name}"),
            ),
            capture::CaptureError::IdpError { error, description } => {
                return make_idp_error(error, description.as_ref());
            }
        };
        make_error(&code, &msg)
    })?;

    if let Some(ref expected) = pending.expected_nonce {
        let received = extract_query_param(captured_url, "nonce").unwrap_or_default();
        relay::verify_nonce(expected, &received)
            .map_err(|_| -> JsValue { make_error("nonce_mismatch", "relay nonce mismatch") })?;
    }

    match pending.flow {
        Flow::Pkce => {
            let token_url = cfg
                .token_url
                .as_deref()
                .ok_or_else(|| make_error("config_invalid", "token_url required for pkce"))?;
            let body_pairs = exchange::build_authorization_code_body(
                &result.value,
                &cfg.client_id,
                &cfg.redirect_uri,
                &pending.verifier,
            );
            let body = exchange::encode_form_body(&body_pairs);

            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"kind".into(), &"exchange".into()).ok();
            js_sys::Reflect::set(&obj, &"url".into(), &token_url.into()).ok();
            js_sys::Reflect::set(&obj, &"body".into(), &body.into()).ok();
            Ok(obj.into())
        }
        Flow::Implicit => {
            let expires_param = cfg.expires_param.as_deref().unwrap_or("expires_in");
            let expires_in = result
                .params
                .get(expires_param)
                .and_then(|v| v.parse::<u64>().ok());
            let expires_at = expiry::compute_expiry(pending.now_secs, expires_in, None);

            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"kind".into(), &"token".into()).ok();
            js_sys::Reflect::set(&obj, &"token".into(), &result.value.into()).ok();
            if let Some(ea) = expires_at {
                js_sys::Reflect::set(&obj, &"expiresAt".into(), &(ea as f64).into()).ok();
            }
            if let Some(s) = result.params.get("scope") {
                js_sys::Reflect::set(&obj, &"scope".into(), &s.clone().into()).ok();
            }
            Ok(obj.into())
        }
    }
}

#[wasm_bindgen(js_name = parseTokenResponse)]
pub fn parse_token_response(
    config_json: &str,
    body: &str,
    now_secs: f64,
) -> Result<JsValue, JsValue> {
    let raw_cfg = parse_oauth_config(config_json)?;
    let _valid = validate_config(raw_cfg)?;
    token_response_to_js(body, now_secs)
}

#[wasm_bindgen(js_name = parseRefreshResponse)]
pub fn parse_refresh_response(body: &str, now_secs: f64) -> Result<JsValue, JsValue> {
    token_response_to_js(body, now_secs)
}

fn token_response_to_js(body: &str, now_secs: f64) -> Result<JsValue, JsValue> {
    let resp = exchange::parse_token_response(body).map_err(|e| -> JsValue {
        let (code, msg) = match &e {
            exchange::ExchangeError::MissingAccessToken => {
                ("missing_access_token", "missing access_token".into())
            }
            exchange::ExchangeError::MissingTokenType => {
                ("invalid_token_type", "token_type must be Bearer".into())
            }
            exchange::ExchangeError::EmptyAccessToken => {
                ("empty_access_token", "empty access_token".into())
            }
            exchange::ExchangeError::InvalidJson(_) => {
                ("invalid_json", "invalid token response JSON".into())
            }
            exchange::ExchangeError::InvalidTokenType => {
                ("invalid_token_type", "token_type must be Bearer".into())
            }
            exchange::ExchangeError::InvalidExpiresIn(val) => {
                ("invalid_expires_in", format!("invalid expires_in: {val}"))
            }
            exchange::ExchangeError::ErrorWithToken => (
                "error_with_token",
                "response contains both error and access_token".into(),
            ),
            exchange::ExchangeError::IdpError { error, description } => {
                return make_idp_error(error, description.as_ref());
            }
        };
        make_error(code, &msg)
    })?;

    let expires_at = expiry::compute_expiry(now_secs as u64, resp.expires_in, None);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(
        &obj,
        &"accessToken".into(),
        &resp.access_token.clone().into(),
    )
    .ok();
    if let Some(rt) = &resp.refresh_token {
        js_sys::Reflect::set(&obj, &"refreshToken".into(), &rt.clone().into()).ok();
    }
    if let Some(ea) = expires_at {
        js_sys::Reflect::set(&obj, &"expiresAt".into(), &(ea as f64).into()).ok();
    }
    if let Some(s) = &resp.scope {
        js_sys::Reflect::set(&obj, &"scope".into(), &s.clone().into()).ok();
    }
    Ok(obj.into())
}

#[wasm_bindgen(js_name = refreshRequest)]
pub fn refresh_request(config_json: &str, refresh_token: &str) -> Result<JsValue, JsValue> {
    let raw_cfg = parse_oauth_config(config_json)?;
    let valid = validate_config(raw_cfg)?;
    let cfg = valid.inner();

    let token_url = cfg
        .token_url
        .as_deref()
        .ok_or_else(|| make_error("config_invalid", "token_url required for refresh"))?;

    let body_pairs = exchange::build_refresh_body(refresh_token, &cfg.client_id);
    let body = exchange::encode_form_body(&body_pairs);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"url".into(), &token_url.into()).ok();
    js_sys::Reflect::set(&obj, &"body".into(), &body.into()).ok();
    Ok(obj.into())
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RefreshInput {
    token_url: String,
    client_id: String,
    refresh_token: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
}

#[wasm_bindgen(js_name = refreshTokenRequest)]
pub fn refresh_token_request(params_json: &str) -> Result<JsValue, JsValue> {
    let mut input: RefreshInput = serde_json::from_str(params_json).map_err(|e| {
        make_error(
            "invalid_params",
            &sanitize_serde_error(&e.to_string()).replace("config", "refresh params"),
        )
    })?;
    let built = exchange::refresh_request(&exchange::RefreshParams {
        token_url: &input.token_url,
        client_id: &input.client_id,
        refresh_token: &input.refresh_token,
        scope: input.scope.as_deref(),
        resource: input.resource.as_deref(),
        client_secret: input.client_secret.as_deref(),
    });
    input.refresh_token.zeroize();
    if let Some(secret) = input.client_secret.as_mut() {
        secret.zeroize();
    }
    let (url, mut body) = built.map_err(|e| make_error("invalid_params", &e))?;
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"url".into(), &url.into()).ok();
    js_sys::Reflect::set(&obj, &"body".into(), &body.as_str().into()).ok();
    body.zeroize();
    Ok(obj.into())
}

#[wasm_bindgen(js_name = scopesSatisfied)]
pub fn scopes_satisfied(granted: &str, requested: &str) -> bool {
    scopes::scopes_satisfied(Some(granted), requested)
}

#[wasm_bindgen(js_name = shouldBackoff)]
pub fn should_backoff(last_failure_secs: f64, now_secs: f64) -> bool {
    backoff::should_backoff(Some(last_failure_secs as u64), now_secs as u64)
}
