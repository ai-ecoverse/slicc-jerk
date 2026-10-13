use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientMetadata {
    pub redirect_uris: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClientInformation {
    pub client_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id_issued_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<u64>,
}

pub fn application_type(redirect_uris: &[String]) -> &'static str {
    let native = redirect_uris.iter().any(|uri| {
        if let Ok(u) = url::Url::parse(uri) {
            let host = u.host_str().unwrap_or("");
            (u.scheme() != "http" && u.scheme() != "https") || is_loopback(host)
        } else {
            false
        }
    });
    if native {
        "native"
    } else {
        "web"
    }
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

pub fn build_dcr_body(redirect_uri: &str, client_name: &str, scope: Option<&str>) -> String {
    let mut map = serde_json::Map::new();
    let uris = vec![serde_json::Value::String(redirect_uri.to_string())];
    map.insert("redirect_uris".into(), serde_json::Value::Array(uris));
    map.insert(
        "token_endpoint_auth_method".into(),
        serde_json::Value::String("none".into()),
    );
    map.insert(
        "grant_types".into(),
        serde_json::Value::Array(vec![
            serde_json::Value::String("authorization_code".into()),
            serde_json::Value::String("refresh_token".into()),
        ]),
    );
    map.insert(
        "response_types".into(),
        serde_json::Value::Array(vec![serde_json::Value::String("code".into())]),
    );
    map.insert(
        "application_type".into(),
        serde_json::Value::String(application_type(&[redirect_uri.to_string()]).into()),
    );
    map.insert(
        "client_name".into(),
        serde_json::Value::String(client_name.into()),
    );
    if let Some(s) = scope {
        map.insert("scope".into(), serde_json::Value::String(s.into()));
    }
    serde_json::Value::Object(map).to_string()
}

pub fn parse_dcr_response(body: &str) -> Result<ClientInformation, String> {
    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|_| "invalid JSON".to_string())?;
    if let Some(error) = map.get("error").and_then(|v| v.as_str()) {
        let error = crate::capture::sanitize_error_description(error);
        let desc = map
            .get("error_description")
            .and_then(|v| v.as_str())
            .map(crate::capture::sanitize_error_description)
            .unwrap_or_default();
        return Err(format!("registration error: {error}: {desc}"));
    }
    let client_id = map
        .get("client_id")
        .and_then(|v| v.as_str())
        .ok_or("missing client_id")?
        .to_string();
    let client_secret = map
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(String::from);
    let client_id_issued_at = map.get("client_id_issued_at").and_then(|v| v.as_u64());
    let client_secret_expires_at = map.get("client_secret_expires_at").and_then(|v| v.as_u64());
    Ok(ClientInformation {
        client_id,
        client_secret,
        client_id_issued_at,
        client_secret_expires_at,
    })
}

pub fn build_revocation_body(token: &str, client_id: &str) -> String {
    let pairs = [("token", token), ("client_id", client_id)];
    crate::exchange::encode_form_body(
        &pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>(),
    )
}

pub fn check_iss(
    received_iss: Option<&str>,
    expected_issuer: &str,
    iss_supported: bool,
) -> Result<(), String> {
    match received_iss {
        Some(iss) => {
            let iss_trimmed = iss.strip_suffix('/').unwrap_or(iss);
            let exp_trimmed = expected_issuer.strip_suffix('/').unwrap_or(expected_issuer);
            if iss_trimmed != exp_trimmed {
                return Err(format!(
                    "iss mismatch: expected {expected_issuer}, got {iss}"
                ));
            }
            Ok(())
        }
        None if iss_supported => Err("iss parameter missing but AS advertises support".into()),
        None => Ok(()),
    }
}
