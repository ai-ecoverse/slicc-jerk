use super::OAuthHost;
use crate::browser::Browser;
use crate::grants::Grants;

pub struct DefaultOAuthHost;

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
        super::http_client::post(url, body)
    }

    fn http_get(&self, url: &str) -> Result<String, String> {
        super::http_client::get(url)
    }

    fn http_post_json(&self, url: &str, body: &str) -> Result<String, String> {
        super::http_client::post_json(url, body)
    }

    fn mcp_probe(&self, url: &str, body: &str) -> super::McpProbeResult {
        super::http_client::mcp_probe(url, body)
    }

    fn sleep(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
}
