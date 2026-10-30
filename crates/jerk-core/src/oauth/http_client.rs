use crate::reach::{Reach, REFUSED};
use std::sync::OnceLock;
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

#[derive(Debug)]
struct Confined(Reach);

impl Resolver for Confined {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let found = DefaultResolver::default().resolve(uri, config, timeout)?;
        let mut kept = self.empty();
        for addr in found.iter().filter(|addr| self.0.allows(addr.ip())) {
            kept.push(*addr);
        }
        if kept.is_empty() {
            return Err(ureq::Error::Io(std::io::Error::other(REFUSED)));
        }
        Ok(kept)
    }
}

fn confined(reach: Reach) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .build();
    ureq::Agent::with_parts(config, DefaultConnector::default(), Confined(reach))
}

fn agent(reach: Option<Reach>) -> &'static ureq::Agent {
    static OPEN: OnceLock<ureq::Agent> = OnceLock::new();
    static PUBLIC: OnceLock<ureq::Agent> = OnceLock::new();
    static LOOPBACK: OnceLock<ureq::Agent> = OnceLock::new();
    match reach {
        None => OPEN.get_or_init(|| {
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .new_agent()
        }),
        Some(Reach::Public) => PUBLIC.get_or_init(|| confined(Reach::Public)),
        Some(Reach::Loopback) => LOOPBACK.get_or_init(|| confined(Reach::Loopback)),
    }
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

pub fn post(reach: Option<Reach>, url: &str, body: &str) -> Result<String, String> {
    let response = agent(reach)
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

pub fn get(reach: Option<Reach>, url: &str) -> Result<String, String> {
    let response = agent(reach)
        .get(url)
        .header("Accept", "application/json")
        .call()
        .map_err(|e| format!("HTTP request failed: {e}"))?;
    response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("cannot read response: {e}"))
}

pub fn post_json(reach: Option<Reach>, url: &str, body: &str) -> Result<String, String> {
    let response = agent(reach)
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

pub fn mcp_probe(reach: Option<Reach>, url: &str, body: &str) -> crate::oauth::McpProbeResult {
    let response = match agent(reach)
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
