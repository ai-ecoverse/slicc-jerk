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
