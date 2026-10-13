use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Unix,
    Windows,
    Wasi,
}

impl Os {
    #[cfg(target_os = "wasi")]
    pub fn current() -> Os {
        Os::Wasi
    }

    #[cfg(windows)]
    pub fn current() -> Os {
        Os::Windows
    }

    #[cfg(not(any(windows, target_os = "wasi")))]
    pub fn current() -> Os {
        Os::Unix
    }
}

#[derive(Clone, Debug)]
pub struct Env {
    pub os: Os,
    pub vars: BTreeMap<String, String>,
}

impl Env {
    pub fn from_process() -> Env {
        Env {
            os: Os::current(),
            vars: std::env::vars().collect(),
        }
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        let value = if self.os == Os::Windows {
            self.vars
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        } else {
            self.vars.get(name)
        };
        value.map(String::as_str).filter(|v| !v.is_empty())
    }

    fn home(&self) -> PathBuf {
        let name = if self.os == Os::Windows {
            "USERPROFILE"
        } else {
            "HOME"
        };
        PathBuf::from(self.var(name).unwrap_or("/"))
    }

    pub fn home_dir(&self) -> PathBuf {
        self.home()
    }

    fn windows_dir(&self, var: &str, fallback: &[&str]) -> PathBuf {
        match self.var(var) {
            Some(dir) => PathBuf::from(dir),
            None => fallback
                .iter()
                .fold(self.home(), |path, part| path.join(part)),
        }
    }

    pub fn system_dir(&self) -> PathBuf {
        if let Some(dir) = self.var("SLICC_JERK_SYSTEM_DIR") {
            return PathBuf::from(dir);
        }
        match self.os {
            Os::Windows => PathBuf::from(self.var("ProgramData").unwrap_or("C:\\ProgramData"))
                .join("slicc-jerk")
                .join("providers.d"),
            _ => PathBuf::from("/etc/slicc-jerk/providers.d"),
        }
    }

    pub fn user_file(&self) -> PathBuf {
        let dir = match self.os {
            Os::Windows => self.windows_dir("APPDATA", &["AppData", "Roaming"]),
            _ => self
                .var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.home().join(".config")),
        };
        dir.join("slicc-jerk").join("providers.toml")
    }

    pub fn explicit_file(&self, flag: Option<&str>) -> Option<PathBuf> {
        flag.or(self.var("SLICC_JERK_CONFIG")).map(PathBuf::from)
    }

    pub fn state_dir(&self) -> PathBuf {
        match self.os {
            Os::Windows => self
                .windows_dir("LOCALAPPDATA", &["AppData", "Local"])
                .join("slicc-jerk"),
            _ => self
                .var("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| self.home().join(".local").join("state"))
                .join("slicc-jerk"),
        }
    }
}
