use std::io::Read;
use std::time::Duration;

pub fn request(
    method: &str,
    url: &str,
    body: &str,
    content_type: &str,
) -> Result<super::McpProbeResult, String> {
    jerk_oauth::config::require_https_or_loopback(url)
        .map_err(|_| "HTTP requires https or loopback".to_string())?;
    let agent = wasix_net::http::Agent::builder()
        .redirects(0)
        .timeout(Duration::from_secs(30))
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .timeout_write(Duration::from_secs(30))
        .build();
    let response = agent
        .request(method, url)
        .header("Content-Type", content_type)
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|_| "HTTP request failed".to_string())?;
    let status = response.status();
    if (300..400).contains(&status) {
        return Err("HTTP redirect refused for OAuth credentials".into());
    }
    let www_authenticate = response.header("www-authenticate").map(String::from);
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read HTTP response".to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("HTTP response exceeds 1 MiB".into());
    }
    let mut body =
        String::from_utf8(bytes).map_err(|_| "HTTP response is not UTF-8".to_string())?;
    if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&body) {
        if value.get("device_code").is_some() {
            value["interval"] = serde_json::json!(value["interval"].as_u64().unwrap_or(5).max(1));
            body = value.to_string();
        }
        if value.get("error").is_some() {
            let error = value["error"]
                .as_str()
                .filter(|code| {
                    matches!(
                        *code,
                        "authorization_pending"
                            | "slow_down"
                            | "expired_token"
                            | "access_denied"
                            | "invalid_grant"
                            | "invalid_client"
                            | "invalid_scope"
                            | "unsupported_grant_type"
                    )
                })
                .unwrap_or("oauth_error");
            body = serde_json::json!({"error": error}).to_string();
        }
    }
    Ok(super::McpProbeResult {
        status,
        www_authenticate,
        body,
    })
}
