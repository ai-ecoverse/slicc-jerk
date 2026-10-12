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

fn parse_oauth_config(input: &str) -> Result<OAuthConfig, JsValue> {
    let trimmed = input.trim_start();
    if trimmed.starts_with('{') {
        serde_json::from_str::<OAuthConfig>(trimmed).map_err(|e| {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"ok".into(), &false.into()).ok();
            js_sys::Reflect::set(
                &obj,
                &"errors".into(),
                &serde_json::to_string(&[format!("json: {e}")])
                    .unwrap_or_default()
                    .into(),
            )
            .ok();
            obj.into()
        })
    } else {
        #[derive(serde::Deserialize)]
        struct Wrapper {
            oauth: OAuthConfig,
        }
        let wrapper: Wrapper = toml_parse(trimmed)?;
        Ok(wrapper.oauth)
    }
}

fn toml_parse<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, JsValue> {
    let value: serde_json::Value = toml_to_json(input).map_err(|e| -> JsValue {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"ok".into(), &false.into()).ok();
        js_sys::Reflect::set(
            &obj,
            &"errors".into(),
            &serde_json::to_string(&[format!("toml: {e}")])
                .unwrap_or_default()
                .into(),
        )
        .ok();
        obj.into()
    })?;
    serde_json::from_value(value).map_err(|e| {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"ok".into(), &false.into()).ok();
        js_sys::Reflect::set(
            &obj,
            &"errors".into(),
            &serde_json::to_string(&[format!("{e}")])
                .unwrap_or_default()
                .into(),
        )
        .ok();
        obj.into()
    })
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
    let cfg: OAuthConfig =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    if random32.len() != 32 {
        return Err(JsValue::from_str("random32 must be 32 bytes"));
    }
    if random16.len() != 16 {
        return Err(JsValue::from_str("random16 must be 16 bytes"));
    }

    let r32: [u8; 32] = random32.try_into().unwrap();
    let r16: [u8; 16] = random16.try_into().unwrap();

    let verifier_str = pkce::verifier(&r32);
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
        "flow": cfg.flow,
        "now_secs": now_secs as u64,
    });

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
    let cfg: OAuthConfig =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    #[derive(serde::Deserialize)]
    struct Pending {
        verifier: String,
        state: String,
        flow: Flow,
        now_secs: u64,
    }

    let pending: Pending =
        serde_json::from_str(pending_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let result = capture::parse_redirect(
        captured_url,
        &cfg.capture.from,
        &cfg.capture.param,
        Some(&pending.state),
    )
    .map_err(|e| -> JsValue {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"kind".into(), &"error".into()).ok();
        let (code, msg) = match &e {
            capture::CaptureError::MissingParam(name) => {
                ("missing_param", format!("missing parameter: {name}"))
            }
            capture::CaptureError::StateMismatch => ("state_mismatch", "state mismatch".into()),
            capture::CaptureError::IdpError { error, description } => (
                "idp_error",
                match description {
                    Some(d) => format!("{error}: {d}"),
                    None => error.clone(),
                },
            ),
        };
        js_sys::Reflect::set(&obj, &"code".into(), &code.into()).ok();
        js_sys::Reflect::set(&obj, &"message".into(), &msg.into()).ok();
        obj.into()
    })?;

    match pending.flow {
        Flow::Pkce => {
            let token_url = cfg
                .token_url
                .as_deref()
                .ok_or_else(|| JsValue::from_str("token_url required for pkce"))?;
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
            let expires_in = cfg
                .expires_param
                .as_deref()
                .and_then(|p| result.params.get(p))
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
    let cfg: OAuthConfig =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let resp = exchange::parse_token_response(body).map_err(|e| -> JsValue {
        let obj = js_sys::Object::new();
        js_sys::Reflect::set(&obj, &"kind".into(), &"error".into()).ok();
        let (code, msg) = match &e {
            exchange::ExchangeError::MissingAccessToken => {
                ("missing_access_token", "missing access_token".into())
            }
            exchange::ExchangeError::InvalidJson(m) => ("invalid_json", m.clone()),
            exchange::ExchangeError::IdpError { error, description } => (
                "idp_error",
                match description {
                    Some(d) => format!("{error}: {d}"),
                    None => error.clone(),
                },
            ),
        };
        js_sys::Reflect::set(&obj, &"code".into(), &code.into()).ok();
        js_sys::Reflect::set(&obj, &"message".into(), &msg.into()).ok();
        obj.into()
    })?;

    let max_age = cfg.timeout.as_deref().and_then(config::parse_timeout);
    let expires_at = expiry::compute_expiry(now_secs as u64, resp.expires_in, max_age);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"accessToken".into(), &resp.access_token.into()).ok();
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
    let cfg: OAuthConfig =
        serde_json::from_str(config_json).map_err(|e| JsValue::from_str(&e.to_string()))?;

    let token_url = cfg
        .token_url
        .as_deref()
        .ok_or_else(|| JsValue::from_str("token_url required for refresh"))?;

    let body_pairs = exchange::build_refresh_body(refresh_token, &cfg.client_id);
    let body = exchange::encode_form_body(&body_pairs);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"url".into(), &token_url.into()).ok();
    js_sys::Reflect::set(&obj, &"body".into(), &body.into()).ok();
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
