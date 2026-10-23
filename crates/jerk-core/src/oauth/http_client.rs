use std::sync::OnceLock;

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent()
    })
}

pub struct Fetched {
    pub status: u16,
    pub location: Option<String>,
    pub body: Vec<u8>,
}

pub fn fetch(
    url: &str,
    headers: &[(&str, &str)],
    cap: u64,
    timeout: std::time::Duration,
) -> Result<Fetched, String> {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    let agent = AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent()
    });
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .config()
        .timeout_global(Some(timeout))
        .build()
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    let status: u16 = response.status().into();
    let location = response
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body = response
        .into_body()
        .with_config()
        .limit(cap)
        .read_to_vec()
        .map_err(|e| format!("cannot read response: {e}"))?;
    Ok(Fetched {
        status,
        location,
        body,
    })
}

pub fn post(url: &str, body: &str) -> Result<String, String> {
    let response = agent()
        .post(url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {e}"))
}

pub fn get(url: &str) -> Result<String, String> {
    let response = agent()
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {e}"))
}

pub fn post_json(url: &str, body: &str) -> Result<String, String> {
    let response = agent()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .send(body.as_bytes())
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {e}"))
}

pub fn mcp_probe(url: &str, body: &str) -> crate::oauth::McpProbeResult {
    let response = match agent()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .send(body.as_bytes())
    {
        Ok(r) => r,
        Err(_) => {
            return crate::oauth::McpProbeResult {
                status: 0,
                www_authenticate: None,
                body: String::new(),
            }
        }
    };
    let status: u16 = response.status().into();
    let www_auth = response
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .map(String::from);
    let body_text = response.into_body().read_to_string().unwrap_or_default();
    crate::oauth::McpProbeResult {
        status,
        www_authenticate: www_auth,
        body: body_text,
    }
}
