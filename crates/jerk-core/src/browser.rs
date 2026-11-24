pub trait Browser: Send + Sync {
    fn open_url(&self, url: &str) -> Result<(), String>;
    fn remote_callback(&self) -> Option<&'static str> {
        None
    }
    fn label(&self) -> String {
        "the system browser".to_string()
    }
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

    fn label(&self) -> String {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "rundll32 url.dll"
        } else {
            "xdg-open"
        };
        format!("the system browser ({opener})")
    }
}

pub struct NoBrowser;

impl Browser for NoBrowser {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("no browser available".into())
    }
}

pub fn open_cdp(found: &crate::browser_select::Found) -> Result<cdp_client::cdp::Cdp, String> {
    cdp_client::cdp::open(found.start.clone(), None)
}

pub struct CdpOpener<'a> {
    pub flag: Option<&'a str>,
    pub seven_origin: Option<&'a str>,
    pub lookup: crate::browser_select::Lookup<'a>,
}

impl crate::oauth::CdpOpener for CdpOpener<'_> {
    fn open(
        &self,
        err: &mut dyn std::io::Write,
    ) -> Result<crate::oauth::CdpResult, crate::oauth::OAuthError> {
        let found = crate::browser_select::resolve(self.flag, self.seven_origin, &self.lookup)?;
        found.open(err)
    }
}

impl crate::oauth::CdpOpener for crate::browser_select::Found {
    fn open(
        &self,
        err: &mut dyn std::io::Write,
    ) -> Result<crate::oauth::CdpResult, crate::oauth::OAuthError> {
        if cfg!(target_os = "wasi")
            && (self.endpoint().starts_with("wss://") || self.endpoint().starts_with("https://"))
        {
            return Err(crate::oauth::OAuthError {
                exit: crate::exit::Exit::Unavailable,
                message: "wss and https are not available for kernel CDP; use SLICC_CDP_URL".into(),
            });
        }
        let _ = writeln!(err, "{}", self.describe());
        let cdp = open_cdp(self).map_err(|message| crate::oauth::OAuthError {
            exit: crate::exit::Exit::Unavailable,
            message: message.trim_end().to_string(),
        })?;
        Ok(crate::oauth::CdpResult(cdp))
    }
}

pub struct CdpBrowser {
    pub found: crate::browser_select::Found,
    pub remote_callback: Option<&'static str>,
}

impl Browser for CdpBrowser {
    fn remote_callback(&self) -> Option<&'static str> {
        self.remote_callback
    }
    fn open_url(&self, url: &str) -> Result<(), String> {
        let failed = |e: String| {
            format!(
                "cannot open the sign-in page in the CDP browser: {}",
                e.trim_end()
            )
        };
        let mut cdp = open_cdp(&self.found).map_err(failed)?;
        cdp.call(
            "Target.createTarget",
            serde_json::json!({ "url": url }),
            None,
        )
        .map_err(failed)?;
        Ok(())
    }

    fn label(&self) -> String {
        format!(
            "the CDP browser {} (via {})",
            cdp_client::sanitize::redact_url(self.found.endpoint()),
            self.found.rule.label()
        )
    }
}
