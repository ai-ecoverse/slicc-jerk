#![cfg_attr(target_os = "wasi", allow(dead_code, unused_imports))]

use crate::cli::Io;
use crate::exit::Exit;
use crate::paths::{Env, Os};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const API_ENV: &str = "SLICC_UPDATE_API_BASE";
const REPO: &str = "ai-ecoverse/slicc-jerk";
const DEFAULT_API: &str = "https://api.github.com";
const SUMS: &str = "SHA256SUMS";
const MAX_JSON: u64 = 4 << 20;
const MAX_SUMS: u64 = 1 << 20;
const MAX_BINARY: u64 = 256 << 20;
const MAX_REDIRECTS: usize = 5;

pub fn usage(command: &str) -> String {
    format!(
        "jerk {command} [--check] [--force]

Replace this jerk with the latest release of {REPO} from GitHub, over https only.
The release binary must match the release's SHA256SUMS and answer --version with
the release version before it replaces this one, and only a writable jerk is replaced.
A copy installed with cargo, npm or slicc install jerk is left to that tool.

Options:
  --check   only report whether a newer release exists
  --force   replace a development build, or reinstall the current release
"
    )
}

pub fn run(command: &str, args: &[String], env: &Env, io: &mut Io<'_>) -> Exit {
    let mut check = false;
    let mut force = false;
    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--force" => force = true,
            "-h" | "--help" => {
                let _ = io.out.write_all(usage(command).as_bytes());
                return Exit::Ok;
            }
            other => {
                let _ = writeln!(
                    io.err,
                    "jerk {command}: unexpected {other}\nRun jerk {command} --help for usage."
                );
                return Exit::Usage;
            }
        }
    }
    native(command, check, force, env, io)
}

#[cfg(target_os = "wasi")]
fn native(command: &str, _check: bool, _force: bool, _env: &Env, io: &mut Io<'_>) -> Exit {
    let _ = writeln!(
        io.err,
        "jerk {command}: not available in the kernel build; seven updates it"
    );
    Exit::Unsupported
}

#[cfg(not(target_os = "wasi"))]
fn native(command: &str, check: bool, force: bool, env: &Env, io: &mut Io<'_>) -> Exit {
    if let Some(base) = env.var(API_ENV) {
        let _ = writeln!(
            io.err,
            "jerk {command}: {API_ENV} overrides the GitHub API with {}",
            shown(base)
        );
    }
    let result = Setup::from_process(env).and_then(|setup| upgrade(&setup, check, force, io.out));
    finish(command, result, io)
}

pub(crate) fn finish(command: &str, result: Result<(), Fail>, io: &mut Io<'_>) -> Exit {
    match result {
        Ok(()) => Exit::Ok,
        Err(Fail::Refuse(message)) => {
            let _ = writeln!(io.err, "jerk {command}: {message}");
            Exit::Usage
        }
        Err(Fail::Error(message)) => {
            let _ = writeln!(io.err, "jerk {command}: {message}");
            Exit::Error
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Fail {
    Refuse(String),
    Error(String),
}

impl From<String> for Fail {
    fn from(message: String) -> Fail {
        Fail::Error(message)
    }
}

pub(crate) fn shown(base: &str) -> String {
    match url::Url::parse(base) {
        Ok(parsed) if parsed.username().is_empty() && parsed.password().is_none() => {
            base.to_string()
        }
        Ok(parsed) => format!(
            "{} (credentials hidden)",
            parsed.origin().ascii_serialization()
        ),
        Err(_) => "an unparseable URL".to_string(),
    }
}

pub(crate) fn allowed_url(text: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(text).map_err(|_| "refusing an unparseable URL".to_string())?;
    let origin = parsed.origin().ascii_serialization();
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!(
            "refusing {origin}: release URLs can't carry credentials"
        ));
    }
    let loopback = match parsed.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
        Some(url::Host::Ipv6(ip)) => ip == std::net::Ipv6Addr::LOCALHOST,
        None => false,
    };
    match parsed.scheme() {
        "https" if parsed.host().is_some() => Ok(parsed),
        "http" if loopback => Ok(parsed),
        _ => Err(format!(
            "refusing {origin}: releases are fetched over https, or plain http only from 127.0.0.1, ::1 or localhost"
        )),
    }
}

pub(crate) fn follow(
    current: &url::Url,
    status: u16,
    location: Option<String>,
) -> Result<url::Url, String> {
    let location = location.ok_or_else(|| format!("HTTP {status} without a Location"))?;
    let next = current
        .join(&location)
        .map_err(|_| "refusing an unparseable redirect".to_string())?;
    if current.scheme() == "https" && next.scheme() != "https" {
        return Err(format!(
            "refusing a redirect from https to {}",
            next.scheme()
        ));
    }
    allowed_url(next.as_str())
}

pub(crate) fn normalize(version: &str) -> &str {
    let trimmed = version.trim();
    trimmed.strip_prefix('v').unwrap_or(trimmed)
}

pub(crate) fn is_release_version(version: &str) -> bool {
    let trimmed = normalize(version);
    !trimmed.is_empty()
        && trimmed
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn segments(version: &str) -> Vec<u64> {
    normalize(version)
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

pub(crate) fn is_development(version: &str) -> bool {
    !is_release_version(version) || segments(version).iter().all(|part| *part == 0)
}

pub(crate) fn is_newer(latest: &str, current: &str) -> bool {
    let (a, b) = (segments(latest), segments(current));
    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return left > right;
        }
    }
    false
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    Report(String),
    Refuse(String),
    Replace,
}

pub(crate) fn plan(current: &str, latest: &str, check: bool, force: bool) -> Plan {
    let dev = is_development(current);
    let current = normalize(current);
    let newer = is_newer(latest, current);
    if check {
        return Plan::Report(if dev {
            format!("latest release: {latest} (this is a development build, {current})\n")
        } else if newer {
            format!("jerk {latest} is available (you have {current}); run `jerk upgrade`\n")
        } else {
            format!("jerk {current} is up to date\n")
        });
    }
    if dev && !force {
        return Plan::Refuse(format!(
            "refusing to replace a development build ({current}) with release {latest}; pass --force to replace it"
        ));
    }
    if !dev && !force && !newer {
        return Plan::Report(format!("jerk {current} is up to date\n"));
    }
    Plan::Replace
}

pub(crate) fn asset_name(os: &str, arch: &str) -> Option<String> {
    let os = match os {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "windows",
        _ => return None,
    };
    let arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let ext = if os == "windows" { ".exe" } else { "" };
    Some(format!("jerk-{os}-{arch}{ext}"))
}

pub(crate) fn parse_checksums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let sum = fields.next()?;
            let name = fields.next()?;
            if fields.next().is_some() {
                return None;
            }
            Some((
                name.trim_start_matches('*').to_string(),
                sum.to_ascii_lowercase(),
            ))
        })
        .collect()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) struct Setup {
    pub exe: PathBuf,
    pub version: String,
    pub asset: Option<String>,
    pub platform: String,
    pub api: String,
    pub cargo_bins: Vec<PathBuf>,
    pub slicc_bins: Vec<PathBuf>,
    pub npm_launcher: bool,
}

const NPM_ADVICE: &str = "jerk was installed with npm; update it with `npm install -g @ai-ecoverse/slicc-jerk@latest`, or in the project that depends on it";
const NPX_ADVICE: &str =
    "jerk runs from npx; run `npx @ai-ecoverse/slicc-jerk@latest` for the latest";
const CARGO_ADVICE: &str = "jerk was installed with cargo; update it with `cargo install --git https://github.com/ai-ecoverse/slicc-jerk jerk`";
const SLICC_ADVICE: &str =
    "jerk was installed by the slicc CLI; update it with `slicc install jerk`";
const APP_ADVICE: &str = "jerk runs inside an app bundle; the app updates it";

pub(crate) fn managed_by(setup: &Setup) -> Option<&'static str> {
    let parts: Vec<String> = setup
        .exe
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.iter().any(|part| part == "_npx") {
        return Some(NPX_ADVICE);
    }
    if setup.npm_launcher || parts.iter().any(|part| part == "node_modules") {
        return Some(NPM_ADVICE);
    }
    for (index, part) in parts.iter().enumerate() {
        if part.ends_with(".app") && parts.get(index + 1).is_some_and(|next| next == "Contents") {
            return Some(APP_ADVICE);
        }
    }
    let dir = setup.exe.parent()?;
    if setup.cargo_bins.iter().any(|bin| bin == dir) {
        return Some(CARGO_ADVICE);
    }
    if setup.slicc_bins.iter().any(|bin| bin == dir) {
        return Some(SLICC_ADVICE);
    }
    None
}

fn real(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn home(env: &Env) -> Option<PathBuf> {
    let home = match env.os {
        Os::Windows => env.var("USERPROFILE").or_else(|| env.var("HOME")),
        _ => env.var("HOME"),
    };
    home.map(PathBuf::from)
}

pub(crate) fn cargo_bins(env: &Env) -> Vec<PathBuf> {
    let mut bins = Vec::new();
    if let Some(dir) = env.var("CARGO_HOME") {
        bins.push(PathBuf::from(dir).join("bin"));
    }
    if let Some(home) = home(env) {
        bins.push(home.join(".cargo").join("bin"));
    }
    bins.into_iter().map(real).collect()
}

pub(crate) fn slicc_bins(env: &Env) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    match env.os {
        Os::Windows => {
            dirs.extend(env.var("LOCALAPPDATA").map(PathBuf::from));
            dirs.extend(env.var("APPDATA").map(PathBuf::from));
        }
        Os::MacOs => {
            dirs.extend(home(env).map(|home| home.join("Library").join("Application Support")))
        }
        _ => match env.var("XDG_DATA_HOME").map(PathBuf::from) {
            Some(dir) if dir.is_absolute() => dirs.push(dir),
            _ => dirs.extend(home(env).map(|home| home.join(".local").join("share"))),
        },
    }
    dirs.into_iter()
        .map(|dir| real(dir.join("slicc").join("bin")))
        .collect()
}

pub(crate) fn parked_path(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_os_string();
    name.push(".old");
    PathBuf::from(name)
}

pub(crate) fn staging_path(exe: &Path) -> PathBuf {
    let stem = exe
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "jerk".to_string());
    let name = match exe.extension() {
        Some(ext) => format!("{stem}.new.{}", ext.to_string_lossy()),
        None => format!("{stem}.new"),
    };
    exe.with_file_name(name)
}

#[cfg(windows)]
pub fn remove_parked() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(parked_path(&real(exe)));
    }
}

#[cfg(not(target_os = "wasi"))]
pub(crate) use native::*;

#[cfg(not(target_os = "wasi"))]
mod native {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    impl Setup {
        pub(crate) fn from_process(env: &Env) -> Result<Setup, Fail> {
            let exe = std::env::current_exe()
                .map_err(|err| Fail::Error(format!("locating this binary: {err}")))?;
            Ok(Setup {
                exe: real(exe),
                version: crate::version().to_string(),
                asset: asset_name(std::env::consts::OS, std::env::consts::ARCH),
                platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
                api: env
                    .var(API_ENV)
                    .unwrap_or(DEFAULT_API)
                    .trim_end_matches('/')
                    .to_string(),
                cargo_bins: cargo_bins(env),
                slicc_bins: slicc_bins(env),
                npm_launcher: env.var("JERK_LAUNCHER") == Some("npm"),
            })
        }
    }

    pub(crate) struct Release {
        pub version: String,
        pub asset: String,
        pub asset_url: String,
        pub sums_url: String,
    }

    pub(crate) fn upgrade(
        setup: &Setup,
        check: bool,
        force: bool,
        out: &mut dyn Write,
    ) -> Result<(), Fail> {
        if let Some(advice) = managed_by(setup) {
            return Err(Fail::Refuse(advice.to_string()));
        }
        let _ = std::fs::remove_file(parked_path(&setup.exe));
        let release = latest(setup)?;
        match plan(&setup.version, &release.version, check, force) {
            Plan::Report(text) => {
                let _ = out.write_all(text.as_bytes());
                return Ok(());
            }
            Plan::Refuse(text) => return Err(Fail::Refuse(text)),
            Plan::Replace => {}
        }
        writable(&setup.exe)?;
        let _ = writeln!(
            out,
            "upgrading jerk {} to {}",
            normalize(&setup.version),
            release.version
        );
        apply(setup, &release)?;
        let _ = writeln!(
            out,
            "upgraded {} to jerk {} (SHA256SUMS verified)",
            setup.exe.display(),
            release.version
        );
        Ok(())
    }

    pub(crate) fn get(
        setup: &Setup,
        url: &str,
        accept: &str,
        cap: u64,
        total: Duration,
    ) -> Result<Vec<u8>, String> {
        let agent = format!("jerk/{}", normalize(&setup.version));
        let mut current = allowed_url(url)?;
        for _ in 0..=MAX_REDIRECTS {
            let mut headers = vec![("User-Agent", agent.as_str())];
            if !accept.is_empty() {
                headers.push(("Accept", accept));
            }
            let response =
                crate::oauth::http_client::fetch(current.as_str(), &headers, cap, total)?;
            match response.status {
                200 => return Ok(response.body),
                301 | 302 | 303 | 307 | 308 => {
                    current = follow(&current, response.status, response.location)?
                }
                status => return Err(format!("HTTP {status}")),
            }
        }
        Err(format!("more than {MAX_REDIRECTS} redirects"))
    }

    pub(crate) fn latest(setup: &Setup) -> Result<Release, Fail> {
        let url = format!("{}/repos/{REPO}/releases/latest", setup.api);
        let body = get(
            setup,
            &url,
            "application/vnd.github+json",
            MAX_JSON,
            Duration::from_secs(60),
        )
        .map_err(|err| format!("asking GitHub for the latest release: {err}"))?;
        let json: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| "the latest release is not valid JSON".to_string())?;
        let tag = json.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
        if !is_release_version(tag) {
            return Err(Fail::Error(
                "the latest release has an unexpected tag".to_string(),
            ));
        }
        let version = normalize(tag).to_string();
        let asset = setup
            .asset
            .clone()
            .ok_or_else(|| format!("no release binary for {}", setup.platform))?;
        let mut asset_url = None;
        let mut sums_url = None;
        for entry in json
            .get("assets")
            .and_then(|a| a.as_array())
            .into_iter()
            .flatten()
        {
            let name = entry.get("name").and_then(|n| n.as_str());
            let link = entry
                .get("browser_download_url")
                .and_then(|l| l.as_str())
                .map(str::to_string);
            if name == Some(asset.as_str()) {
                asset_url = link;
            } else if name == Some(SUMS) {
                sums_url = link;
            }
        }
        let asset_url = asset_url.ok_or_else(|| format!("release {version} has no {asset}"))?;
        let sums_url = sums_url.ok_or_else(|| format!("release {version} has no {SUMS}"))?;
        Ok(Release {
            version,
            asset,
            asset_url,
            sums_url,
        })
    }

    #[cfg(unix)]
    pub(crate) fn can_write(path: &Path) -> bool {
        use std::os::unix::ffi::OsStrExt;
        let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
    }

    #[cfg(not(unix))]
    pub(crate) fn can_write(path: &Path) -> bool {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                let probe = path.join(format!(".jerk-upgrade-{}", std::process::id()));
                let created = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&probe)
                    .is_ok();
                let _ = std::fs::remove_file(&probe);
                created
            }
            Ok(meta) => !meta.permissions().readonly(),
            Err(_) => false,
        }
    }

    pub(crate) fn writable(exe: &Path) -> Result<(), Fail> {
        if !can_write(exe) {
            return Err(Fail::Error(format!(
                "{} is not writable; rerun with permission to write it, or reinstall with install.sh",
                exe.display()
            )));
        }
        let dir = exe.parent().unwrap_or(Path::new("."));
        if !can_write(dir) {
            return Err(Fail::Error(format!(
                "{} is not writable, so {} can't be replaced; rerun with permission to write it, or reinstall with install.sh",
                dir.display(),
                exe.display()
            )));
        }
        Ok(())
    }

    pub(crate) fn apply(setup: &Setup, release: &Release) -> Result<(), String> {
        let sums = get(
            setup,
            &release.sums_url,
            "",
            MAX_SUMS,
            Duration::from_secs(60),
        )
        .map_err(|err| format!("downloading {SUMS}: {err}"))?;
        let want = parse_checksums(&String::from_utf8_lossy(&sums))
            .into_iter()
            .find(|(name, _)| *name == release.asset)
            .map(|(_, sum)| sum)
            .ok_or_else(|| format!("{SUMS} has no entry for {}", release.asset))?;
        let binary = get(
            setup,
            &release.asset_url,
            "",
            MAX_BINARY,
            Duration::from_secs(600),
        )
        .map_err(|err| format!("downloading {}: {err}", release.asset))?;
        if sha256_hex(&binary) != want {
            return Err(format!(
                "{} does not match its SHA-256 in {SUMS}",
                release.asset
            ));
        }
        let staging = staging_path(&setup.exe);
        let result = stage(&staging, &binary)
            .and_then(|()| verify(&staging, &release.version, Duration::from_secs(30)))
            .and_then(|()| swap(&staging, &setup.exe));
        if result.is_err() {
            let _ = std::fs::remove_file(&staging);
        }
        result
    }

    pub(crate) fn stage(staging: &Path, binary: &[u8]) -> Result<(), String> {
        let _ = std::fs::remove_file(staging);
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging)
            .and_then(|mut file| {
                file.write_all(binary)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    file.set_permissions(std::fs::Permissions::from_mode(0o755))?;
                }
                file.sync_all()
            });
        written.map_err(|err| format!("writing {}: {err}", staging.display()))
    }

    pub(crate) fn verify(staging: &Path, version: &str, limit: Duration) -> Result<(), String> {
        let failed =
            |detail: String| format!("the downloaded binary failed to run --version{detail}");
        let mut child = Command::new(staging)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| failed(format!(": {err}")))?;
        let deadline = Instant::now() + limit;
        while child
            .try_wait()
            .map_err(|err| failed(format!(": {err}")))?
            .is_none()
        {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the downloaded binary did not answer --version in {} seconds",
                    limit.as_secs()
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let output = child
            .wait_with_output()
            .map_err(|err| failed(format!(": {err}")))?;
        if !output.status.success() {
            return Err(failed(format!(" ({})", output.status)));
        }
        let got = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let want = format!("jerk {version}");
        if got != want {
            let got: String = got.chars().take(64).collect();
            return Err(format!(
                "the downloaded binary reports {got:?}, expected {want:?}"
            ));
        }
        Ok(())
    }

    pub(crate) fn swap(staging: &Path, exe: &Path) -> Result<(), String> {
        #[cfg(windows)]
        let parked = park(exe)?;
        let renamed = std::fs::rename(staging, exe);
        #[cfg(windows)]
        {
            if renamed.is_err() && parked {
                let _ = std::fs::rename(parked_path(exe), exe);
            }
        }
        renamed.map_err(|err| format!("replacing {}: {err}", exe.display()))
    }

    #[cfg(windows)]
    fn park(exe: &Path) -> Result<bool, String> {
        if !exe.exists() {
            return Ok(false);
        }
        let parked = parked_path(exe);
        let _ = std::fs::remove_file(&parked);
        std::fs::rename(exe, &parked)
            .map_err(|err| format!("moving the running binary aside: {err}"))?;
        Ok(true)
    }
}

#[cfg(all(test, jerk_unit))]
mod unit {
    include!("../../../test/unit/upgrade_tests.rs");
}
