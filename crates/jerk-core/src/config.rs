use crate::paths::Env;
use jerk_oauth::config::OAuthConfig;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
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
pub struct Lookup {
    pub run: Vec<String>,
    #[serde(default)]
    pub items: String,
    pub prefix: String,
    pub id: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookup: Option<Lookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_version: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub interactive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub link: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub oauth: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VerbChain(pub Vec<Verb>);

impl VerbChain {
    pub fn first(&self) -> Option<&Verb> {
        self.0.first()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Verb> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for VerbChain {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.len() == 1 {
            self.0[0].serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for VerbChain {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = toml::Value::deserialize(deserializer)?;
        match &value {
            toml::Value::Array(arr) => {
                let mut verbs = Vec::with_capacity(arr.len());
                for item in arr {
                    let verb: Verb = item.clone().try_into().map_err(serde::de::Error::custom)?;
                    verbs.push(verb);
                }
                Ok(VerbChain(verbs))
            }
            toml::Value::Table(_) => {
                let verb: Verb = value.try_into().map_err(serde::de::Error::custom)?;
                Ok(VerbChain(vec![verb]))
            }
            _ => Err(serde::de::Error::custom(
                "expected a table or an array of tables",
            )),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provider {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cors_origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub get: Option<VerbChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<VerbChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<VerbChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<VerbChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create: Option<VerbChain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drop: Option<VerbChain>,
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
    Mcp,
    System,
    User,
    Explicit,
}

impl Layer {
    pub fn label(&self) -> &'static str {
        match self {
            Layer::BuiltIn => "built-in",
            Layer::Mcp => "mcp.json",
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
    pub oauth_config: Option<OAuthConfig>,
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
    let (mcp_entries, mcp_problems) = crate::mcp_config::load_mcp_providers(env);
    config.problems.extend(mcp_problems);
    for entry in mcp_entries {
        if !config.providers.contains_key(&entry.id) {
            config.providers.insert(entry.id.clone(), entry);
        }
    }
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
            let raw = e.message().to_string();
            let sanitized =
                if raw.contains("unknown") || raw.contains("invalid") || raw.contains("expected") {
                    sanitize_oauth_serde_error(&raw)
                } else {
                    raw
                };
            config.problems.push(Problem {
                level: Level::Error,
                source: source.to_string(),
                provider: None,
                message: sanitized,
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
        let problems_before = config.problems.len();
        let oauth_config = parse_oauth(&id, &provider, &source, &mut config.problems);
        let oauth_failed = config.problems[problems_before..]
            .iter()
            .any(|p| p.level == Level::Error);
        if oauth_failed {
            continue;
        }
        config.providers.insert(
            id.clone(),
            Entry {
                id,
                provider,
                source: source.clone(),
                oauth_config,
            },
        );
    }
}

fn sanitize_oauth_serde_error(msg: &str) -> String {
    if msg.contains("unknown variant") {
        return "invalid configuration".into();
    }
    for (needle, kind) in [
        ("unknown field `", "invalid"),
        ("missing field `", "missing"),
    ] {
        if let Some(raw) = extract_between(msg, needle, "`") {
            return match jerk_oauth::config::safe_field_name(raw) {
                Some(name) => format!("{kind} field {name}"),
                None => format!("{kind} field"),
            };
        }
    }
    "invalid configuration".into()
}

fn extract_between<'a>(haystack: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    let start = haystack.find(prefix)? + prefix.len();
    let rest = &haystack[start..];
    let end = rest.find(suffix)?;
    Some(&rest[..end])
}

fn parse_oauth(
    id: &str,
    provider: &Provider,
    source: &Source,
    problems: &mut Vec<Problem>,
) -> Option<OAuthConfig> {
    let table = provider.oauth.as_ref()?;
    let value = toml::Value::Table(table.clone());
    let config: OAuthConfig = match value.try_into() {
        Ok(c) => c,
        Err(e) => {
            let raw = e.to_string();
            let sanitized = sanitize_oauth_serde_error(&raw);
            problems.push(Problem {
                level: Level::Error,
                source: source.to_string(),
                provider: Some(id.to_string()),
                message: format!("oauth: {sanitized}"),
            });
            return None;
        }
    };
    let errors = jerk_oauth::config::validate(&config);
    if !errors.is_empty() {
        for err in errors {
            problems.push(Problem {
                level: Level::Error,
                source: source.to_string(),
                provider: Some(id.to_string()),
                message: format!("oauth: {err}"),
            });
        }
        return None;
    }
    Some(config)
}

pub fn validate(id: &str, provider: &Provider, source: &Source) -> Vec<Problem> {
    let mut messages = Vec::new();
    let is_mcp = id.starts_with("mcp:");
    if !valid_id(id) {
        messages.push(
            "the id must be lower-case letters, digits and dashes, starting with a letter or digit, or mcp:<name>"
                .to_string(),
        );
    }
    if provider.locked && source.layer != Layer::System {
        messages.push("locked only applies in system files".to_string());
    }
    if provider.env.is_empty() && !is_mcp {
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
            messages.push("max_age is not a valid duration such as 50m".to_string());
        }
    }
    if let Some(value) = &provider.store {
        if value != "keyring" && value != "file" {
            messages.push("store must be keyring or file".to_string());
        }
    }
    if let Some(value) = &provider.lease_via {
        if value != "get" && value != "create" {
            messages.push("lease_via must be get or create".to_string());
        }
        let revocable = provider
            .create
            .as_ref()
            .and_then(|c| c.first())
            .is_some_and(|c| c.parse.is_some() && (c.id.is_some() || c.lookup.is_some()));
        if value == "create" && !revocable {
            messages.push(
                "lease_via = \"create\" needs a create with parse and an id or lookup, so the lease can be revoked"
                    .to_string(),
            );
        }
    }
    let has_cli_get = provider
        .get
        .as_ref()
        .is_some_and(|c| c.iter().any(|v| !v.oauth));
    if !has_cli_get && provider.oauth.is_none() && !is_mcp {
        messages.push("needs a get command or an oauth block".to_string());
    }
    for (name, chain) in verb_chains(provider) {
        for (i, verb) in chain.iter().enumerate() {
            validate_verb(name, verb, i, chain.len(), provider, &mut messages);
        }
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

pub fn verb_chains(provider: &Provider) -> Vec<(&'static str, &VerbChain)> {
    [
        ("get", &provider.get),
        ("list", &provider.list),
        ("scopes", &provider.scopes),
        ("user", &provider.user),
        ("create", &provider.create),
        ("drop", &provider.drop),
    ]
    .into_iter()
    .filter_map(|(name, chain)| chain.as_ref().map(|c| (name, c)))
    .collect()
}

pub fn verbs(provider: &Provider) -> Vec<(&'static str, &Verb)> {
    verb_chains(provider)
        .into_iter()
        .flat_map(|(name, chain)| chain.iter().map(move |v| (name, v)))
        .collect()
}

fn validate_verb(
    name: &str,
    verb: &Verb,
    step: usize,
    chain_len: usize,
    provider: &Provider,
    messages: &mut Vec<String>,
) {
    let label = if chain_len > 1 {
        format!("{name}[{step}]")
    } else {
        name.to_string()
    };
    if verb.oauth {
        if verb.run.is_some() {
            messages.push(format!("{label}: oauth = true cannot have run"));
        }
        if verb.read.is_some() {
            messages.push(format!("{label}: oauth = true cannot have read"));
        }
        if provider.oauth.is_none() {
            messages.push(format!("{label}: oauth = true but no [oauth] block"));
        }
        return;
    }
    let step_requires = verb.requires.as_ref().or(provider.requires.as_ref());
    match (&verb.run, &verb.read) {
        (Some(_), Some(_)) => messages.push(format!("{label}: use either run or read, not both")),
        (None, None) => messages.push(format!("{label}: needs run or read")),
        (Some(run), None) if run.is_empty() => messages.push(format!("{label}: run is empty")),
        (Some(_), None) if step_requires.is_none() => {
            messages.push(format!("{label}: run needs requires to name the CLI"))
        }
        _ => {}
    }
    if verb.read.is_some() && name != "get" {
        messages.push(format!("{label}: read only applies to get"));
    }
    if let Some(lookup) = &verb.lookup {
        if name != "create" || verb.parse.is_none() {
            messages.push(format!(
                "{label}: lookup only applies to a create with parse"
            ));
        }
        if lookup.run.is_empty() {
            messages.push(format!("{label}: lookup.run is empty"));
        }
        for pointer in [&lookup.items, &lookup.prefix, &lookup.id] {
            if !pointer.is_empty() && !pointer.starts_with('/') {
                messages.push(format!(
                    "{label}: lookup pointer {pointer:?} must start with /"
                ));
            }
        }
    }
    if let Some(version) = &verb.min_version {
        if parse_version(version).is_none() {
            messages.push(format!(
                "{label}: min_version {version:?} must look like 11.6.3"
            ));
        }
    }
    if let Some(id) = &verb.id {
        if name != "create" || !id.starts_with("json:") || !valid_parse(id) {
            messages.push(format!(
                "{label}: id {id:?} must be json:<pointer>, on create only"
            ));
        }
        if verb
            .parse
            .as_deref()
            .is_none_or(|p| !p.starts_with("json:"))
        {
            messages.push(format!("{label}: id needs parse = \"json:<pointer>\""));
        }
    }
    if let Some(parse) = &verb.parse {
        if !valid_parse(parse) {
            messages.push(format!(
                "{label}: parse {parse:?} must be text, jwt or json:<pointer>"
            ));
        }
    }
    if let Some(expiry) = &verb.expiry {
        if expiry != "jwt" {
            messages.push(format!("{label}: expiry {expiry:?} must be jwt"));
        }
    }
    if let Some(timeout) = &verb.timeout {
        if parse_duration(timeout).is_none() {
            messages.push(format!(
                "{label}: timeout {timeout:?} is not a duration such as 10s"
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
                        messages.push(format!("{label}: unknown placeholder {{{var}}}"));
                    }
                }
            }
            Err(e) => messages.push(format!("{label}: {e} in {text:?}")),
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
    if let Some(rest) = id.strip_prefix("mcp:") {
        if let Some(url_part) = rest.strip_prefix("url:") {
            return url::Url::parse(url_part).is_ok();
        }
        return valid_mcp_name(rest);
    }
    let mut chars = id.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
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

pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter_map(|word| {
            let mut parts = word.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next()?.parse().ok()?;
            Some((major, minor, patch))
        })
        .next()
}
