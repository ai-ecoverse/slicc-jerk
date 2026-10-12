use super::OAuthHost;
use crate::browser::Browser;
use crate::grants::Grants;

pub struct DefaultOAuthHost;

impl OAuthHost for DefaultOAuthHost {
    fn browser(&self) -> &dyn Browser {
        &crate::browser::SystemBrowser
    }

    fn grants(&self, store: Option<&str>) -> Box<dyn Grants> {
        if store == Some("file") {
            let dir = crate::paths::Env::from_process().state_dir();
            Box::new(crate::grants::file_store::FileStore::new(dir))
        } else {
            Box::new(crate::grants::keyring_store::KeyringStore::new())
        }
    }

    fn http_post(&self, url: &str, body: &str) -> Result<String, String> {
        super::http_client::post(url, body)
    }

    fn sleep(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
}
