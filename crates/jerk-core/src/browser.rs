pub trait Browser: Send + Sync {
    fn open_url(&self, url: &str) -> Result<(), String>;
}

#[cfg(not(target_os = "wasi"))]
pub struct SystemBrowser;

#[cfg(not(target_os = "wasi"))]
impl Browser for SystemBrowser {
    fn open_url(&self, url: &str) -> Result<(), String> {
        let result = if cfg!(target_os = "macos") {
            std::process::Command::new("open").arg(url).status()
        } else if cfg!(target_os = "windows") {
            std::process::Command::new("rundll32")
                .args(["url.dll,FileProtocolHandler", url])
                .status()
        } else {
            std::process::Command::new("xdg-open").arg(url).status()
        };
        match result {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(format!("browser exited with {status}")),
            Err(e) => Err(format!("cannot open browser: {e}")),
        }
    }
}

pub struct NoBrowser;

impl Browser for NoBrowser {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("no browser available".into())
    }
}
