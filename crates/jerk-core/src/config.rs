use crate::paths::Env;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub ini: String,
    pub key: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verb {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<Read>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parse: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub link: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get: Option<Verb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<Verb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Verb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<Verb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<Verb>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<toml::Table>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    providers: BTreeMap<String, Provider>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    BuiltIn,
    System,
    User,
    Explicit,
}

impl Layer {
    pub fn label(&self) -> &'static str {
        match self {
            Layer::BuiltIn => "built-in",
            Layer::System => "system",
            Layer::User => "user",
            Layer::Explicit => "explicit",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub layer: Layer,
    pub path: Option<PathBuf>,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(f, "{}", path.display()),
            None => write!(f, "{}", self.layer.label()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub provider: Provider,
    pub source: Source,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub level: Level,
    pub source: String,
    pub provider: Option<String>,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        match &self.provider {
            Some(id) => write!(f, "{level}: {}: {id}: {}", self.source, self.message),
            None => write!(f, "{level}: {}: {}", self.source, self.message),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub providers: BTreeMap<String, Entry>,
    pub problems: Vec<Problem>,
}

impl Config {
    pub fn has_errors(&self) -> bool {
        self.problems.iter().any(|p| p.level == Level::Error)
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.providers.get(id)
    }
}

pub const TEMPLATE_VARS: [&str; 5] = ["scope", "id", "name", "expires", "user"];

pub fn layer_files(env: &Env, explicit: Option<&str>) -> (Vec<Source>, Vec<Problem>) {
    let mut sources = Vec::new();
    let mut problems = Vec::new();
    let system = env.system_dir();
    match std::fs::read_dir(&system) {
        Ok(dir) => {
            let mut files: Vec<PathBuf> = dir
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "toml"))
                .collect();
            files.sort();
            sources.extend(files.into_iter().map(|path| Source {
                layer: Layer::System,
                path: Some(path),
            }));
        }
        Err(e) if missing(&e) => {}
        Err(e) => problems.push(Problem {
            level: Level::Error,
            source: system.display().to_string(),
            provider: None,
            message: format!("cannot read the system directory: {e}"),
        }),
    }
    sources.push(Source {
        layer: Layer::User,
        path: Some(env.user_file()),
    });
    if let Some(path) = env.explicit_file(explicit) {
        sources.push(Source {
            layer: Layer::Explicit,
            path: Some(path),
        });
    }
    (sources, problems)
}

#[cfg(target_os = "wasi")]
pub(crate) fn missing(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound || e.raw_os_error() == Some(76)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn missing(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::NotFound
}

pub fn load(env: &Env, explicit: Option<&str>) -> Config {
    let mut config = Config::default();
    merge(&mut config, builtin_source(), crate::builtin::BUILTIN);
    let (sources, problems) = layer_files(env, explicit);
    config.problems.extend(problems);
    for source in sources {
        let path = source.path.clone().unwrap_or_default();
        match std::fs::read_to_string(&path) {
            Ok(text) => merge(&mut config, source, &text),
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound && source.layer != Layer::Explicit => {}
            Err(e) => config.problems.push(Problem {
                level: Level::Error,
                source: source.to_string(),
                provider: None,
                message: format!("cannot read: {e}"),
            }),
        }
    }
    config
}

fn builtin_source() -> Source {
    Source {
        layer: Layer::BuiltIn,
        path: None,
    }
}

pub fn merge(config: &mut Config, source: Source, text: &str) {
    let file: File = match toml::from_str(text) {
        Ok(file) => file,
        Err(e) => {
            config.problems.push(Problem {
                level: Level::Error,
                source: source.to_string(),
                provider: None,
                message: e.message().to_string(),
            });
            return;
        }
    };
    for (id, provider) in file.providers {
        let problems = validate(&id, &provider, &source);
        let failed = problems.iter().any(|p| p.level == Level::Error);
        config.problems.extend(problems);
        if failed {
            continue;
        }
        if let Some(existing) = config.providers.get(&id) {
            if existing.provider.locked {
                config.problems.push(Problem {
                    level: Level::Warning,
                    source: source.to_string(),
                    provider: Some(id.clone()),
                    message: format!("locked by {}; this definition is ignored", existing.source),
                });
                continue;
            }
        }
        config.providers.insert(
            id.clone(),
            Entry {
                id,
                provider,
                source: source.clone(),
            },
        );
    }
}

pub fn validate(id: &str, provider: &Provider, source: &Source) -> Vec<Problem> {
    let mut messages = Vec::new();
    if !valid_id(id) {
        messages.push(
            "the id must be lower-case letters, digits and dashes, starting with a letter or digit"
                .to_string(),
        );
    }
    if provider.locked && source.layer != Layer::System {
        messages.push("locked only applies in system files".to_string());
    }
    if provider.env.is_empty() {
        messages.push("env needs at least one name".to_string());
    }
    for name in &provider.env {
        if !valid_env(name) {
            messages.push(format!(
                "env name {name:?} is not a valid environment variable name"
            ));
        }
    }
    for domain in &provider.domains {
        if !valid_domain(domain) {
            messages.push(format!("domain {domain:?} must be an exact host name"));
        }
    }
    if let Some(value) = &provider.max_age {
        if parse_duration(value).is_none() {
            messages.push(format!("max_age {value:?} is not a duration such as 50m"));
        }
    }
    if let Some(value) = &provider.store {
        if value != "keyring" && value != "file" {
            messages.push(format!("store {value:?} must be keyring or file"));
        }
    }
    if let Some(value) = &provider.lease_via {
        if value != "get" && value != "create" {
            messages.push(format!("lease_via {value:?} must be get or create"));
        }
    }
    if provider.get.is_none() && provider.oauth.is_none() {
        messages.push("needs a get command or an oauth block".to_string());
    }
    for (name, verb) in verbs(provider) {
        validate_verb(name, verb, provider, &mut messages);
    }
    messages
        .into_iter()
        .map(|message| Problem {
            level: Level::Error,
            source: source.to_string(),
            provider: Some(id.to_string()),
            message,
        })
        .collect()
}

pub fn verbs(provider: &Provider) -> Vec<(&'static str, &Verb)> {
    [
        ("get", &provider.get),
        ("list", &provider.list),
        ("scopes", &provider.scopes),
        ("create", &provider.create),
        ("drop", &provider.drop),
    ]
    .into_iter()
    .filter_map(|(name, verb)| verb.as_ref().map(|v| (name, v)))
    .collect()
}

fn validate_verb(name: &str, verb: &Verb, provider: &Provider, messages: &mut Vec<String>) {
    match (&verb.run, &verb.read) {
        (Some(_), Some(_)) => messages.push(format!("{name}: use either run or read, not both")),
        (None, None) => messages.push(format!("{name}: needs run or read")),
        (Some(run), None) if run.is_empty() => messages.push(format!("{name}: run is empty")),
        (Some(_), None) if provider.requires.is_none() => {
            messages.push(format!("{name}: run needs requires to name the CLI"))
        }
        _ => {}
    }
    if verb.read.is_some() && name != "get" {
        messages.push(format!("{name}: read only applies to get"));
    }
    if let Some(parse) = &verb.parse {
        if !valid_parse(parse) {
            messages.push(format!(
                "{name}: parse {parse:?} must be text, jwt or json:<pointer>"
            ));
        }
    }
    if let Some(expiry) = &verb.expiry {
        if expiry != "jwt" {
            messages.push(format!("{name}: expiry {expiry:?} must be jwt"));
        }
    }
    if let Some(timeout) = &verb.timeout {
        if parse_duration(timeout).is_none() {
            messages.push(format!(
                "{name}: timeout {timeout:?} is not a duration such as 10s"
            ));
        }
    }
    let mut texts: Vec<&str> = verb.run.iter().flatten().map(String::as_str).collect();
    if let Some(read) = &verb.read {
        texts.push(&read.ini);
        texts.push(&read.key);
    }
    if let Some(parse) = &verb.parse {
        texts.push(parse);
    }
    for text in texts {
        match placeholders(text) {
            Ok(names) => {
                for var in names {
                    if !provider.params.contains_key(&var) && !TEMPLATE_VARS.contains(&var.as_str())
                    {
                        messages.push(format!("{name}: unknown placeholder {{{var}}}"));
                    }
                }
            }
            Err(e) => messages.push(format!("{name}: {e} in {text:?}")),
        }
    }
}

pub fn placeholders(text: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'$' if bytes.get(i + 1) == Some(&b'{') => {
                let end = text[i..].find('}').ok_or("an unclosed ${")?;
                i += end + 1;
            }
            b'{' => {
                let end = text[i..].find('}').ok_or("an unclosed {")?;
                let name = &text[i + 1..i + end];
                if name.is_empty() || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                    return Err(format!("a bad placeholder {{{name}}}"));
                }
                names.push(name.to_string());
                i += end + 1;
            }
            b'}' => return Err("a stray }".to_string()),
            _ => i += 1,
        }
    }
    Ok(names)
}

pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn valid_env(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

pub fn valid_parse(parse: &str) -> bool {
    match parse {
        "text" | "jwt" => true,
        other => other
            .strip_prefix("json:")
            .is_some_and(|pointer| pointer.is_empty() || pointer.starts_with('/')),
    }
}

pub fn parse_duration(text: &str) -> Option<std::time::Duration> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let n: u64 = number.parse().ok()?;
    let ms = match unit {
        "ms" => n,
        "s" => n.checked_mul(1000)?,
        "m" => n.checked_mul(60_000)?,
        "h" => n.checked_mul(3_600_000)?,
        "d" => n.checked_mul(86_400_000)?,
        _ => return None,
    };
    Some(std::time::Duration::from_millis(ms))
}
