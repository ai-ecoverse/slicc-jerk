use super::OAuthHost;
use crate::browser::Browser;
use crate::grants::Grants;
use crate::reach::{Reach, REFUSED};
use std::net::ToSocketAddrs;

#[derive(Default)]
pub struct DefaultOAuthHost {
    reach: Option<Reach>,
}

impl OAuthHost for DefaultOAuthHost {
    fn browser(&self) -> &dyn Browser {
        &crate::browser::SystemBrowser
    }

    fn grants(&self, store: Option<&str>) -> Box<dyn Grants> {
        let env = crate::paths::Env::from_process();
        let store = store
            .map(String::from)
            .or_else(|| env.var("SLICC_JERK_STORE").map(String::from));
        if store.as_deref() == Some("file") {
            let dir = env.state_dir();
            Box::new(crate::grants::file_store::FileStore::new(dir))
        } else {
            Box::new(crate::grants::keyring_store::KeyringStore::new())
        }
    }

    fn http_post(&self, url: &str, body: &str) -> Result<String, String> {
        super::http_client::post(self.reach, url, body)
    }

    fn http_get(&self, url: &str) -> Result<String, String> {
        super::http_client::get(self.reach, url)
    }

    fn http_post_json(&self, url: &str, body: &str) -> Result<String, String> {
        super::http_client::post_json(self.reach, url, body)
    }

    fn mcp_probe(&self, url: &str, body: &str) -> super::McpProbeResult {
        super::http_client::mcp_probe(self.reach, url, body)
    }

    fn sleep(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }

    fn confine(&self, reach: Reach) -> Box<dyn OAuthHost> {
        Box::new(DefaultOAuthHost { reach: Some(reach) })
    }

    fn reachable(&self, url: &str) -> Result<(), String> {
        match self.reach {
            Some(reach) => addresses_allowed(reach, url),
            None => Ok(()),
        }
    }
}

fn addresses_allowed(reach: Reach, url: &str) -> Result<(), String> {
    let refused = || REFUSED.to_string();
    let parsed = url::Url::parse(url).map_err(|_| refused())?;
    let port = parsed.port_or_known_default().ok_or_else(refused)?;
    let ips: Vec<std::net::IpAddr> = match parsed.host() {
        Some(url::Host::Ipv4(ip)) => vec![ip.into()],
        Some(url::Host::Ipv6(ip)) => vec![ip.into()],
        Some(url::Host::Domain(name)) => (name, port)
            .to_socket_addrs()
            .map_err(|_| "this host does not resolve".to_string())?
            .map(|addr| addr.ip())
            .collect(),
        None => Vec::new(),
    };
    if !ips.is_empty() && ips.iter().all(|ip| reach.allows(*ip)) {
        Ok(())
    } else {
        Err(refused())
    }
}
