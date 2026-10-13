use crate::config::{Entry, Layer, Provider, Source};
use crate::paths::Env;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct McpOAuthSettings {
    #[serde(rename = "clientId")]
    client_id: Option<String>,
    #[serde(rename = "clientSecret")]
    client_secret: Option<String>,
    #[serde(rename = "clientName")]
    client_name: Option<String>,
    scope: Option<String>,
    #[serde(rename = "callbackPort")]
    callback_port: Option<u16>,
    #[serde(rename = "callbackUrl")]
    callback_url: Option<String>,
    #[serde(rename = "clientRegistration")]
    client_registration: Option<String>,
    #[serde(rename = "authServerMetadataUrl")]
    auth_server_metadata_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct McpServerEntry {
    #[serde(default)]
    r#type: Option<String>,
    url: Option<String>,
    oauth: Option<McpOAuthSettings>,
    #[serde(flatten)]
    _rest: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McpJson {
    #[serde(rename = "mcpServers")]
    mcp_servers: Option<BTreeMap<String, McpServerEntry>>,
}

pub fn pi_dir(env: &Env) -> PathBuf {
    env.var("PI_AGENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = env.home_dir();
            home.join(".pi").join("agent")
        })
}

pub fn mcp_json_path(env: &Env) -> PathBuf {
    env.var("PI_MCP_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| pi_dir(env).join("mcp.json"))
}

pub fn mcp_auth_json_path(env: &Env) -> PathBuf {
    env.var("PI_MCP_AUTH_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| pi_dir(env).join("mcp-auth.json"))
}

pub fn env_name_for(name: &str) -> String {
    let upper: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("MCP_{upper}_TOKEN")
}

pub fn mcp_id(name: &str) -> String {
    format!("mcp:{name}")
}

#[derive(Clone, Debug)]
pub struct McpProviderInfo {
    pub id: String,
    pub server_name: String,
    pub url: String,
    pub scope: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_name: Option<String>,
    pub callback_port: Option<u16>,
    pub callback_url: Option<String>,
    pub client_registration: Option<String>,
    pub auth_server_metadata_url: Option<String>,
}

pub fn load_mcp_providers(env: &Env) -> (Vec<Entry>, Vec<crate::config::Problem>) {
    let path = mcp_json_path(env);
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if crate::config::missing(&e) => return (entries, problems),
        Err(e) => {
            problems.push(crate::config::Problem {
                level: crate::config::Level::Warning,
                source: path.display().to_string(),
                provider: None,
                message: format!("cannot read: {e}"),
            });
            return (entries, problems);
        }
    };
    let mcp: McpJson = match serde_json::from_str(&text) {
        Ok(m) => m,
        Err(e) => {
            problems.push(crate::config::Problem {
                level: crate::config::Level::Warning,
                source: path.display().to_string(),
                provider: None,
                message: format!("cannot parse: {e}"),
            });
            return (entries, problems);
        }
    };
    let servers = mcp.mcp_servers.unwrap_or_default();
    for (name, server) in servers {
        let is_http = server
            .r#type
            .as_deref()
            .map(|t| t == "http")
            .unwrap_or(server.url.is_some());
        if !is_http {
            continue;
        }
        let Some(url) = server.url.as_deref() else {
            continue;
        };
        let id = mcp_id(&name);
        let env_var = env_name_for(&name);
        let mut provider = Provider {
            kind: Some("mcp".into()),
            name: Some(name.clone()),
            env: vec![env_var],
            url: Some(url.to_string()),
            ..Provider::default()
        };
        if let Some(ref oauth) = server.oauth {
            if let Some(ref scope) = oauth.scope {
                provider.params.insert("scope".into(), scope.clone());
            }
            if let Some(ref cid) = oauth.client_id {
                provider.params.insert("client_id".into(), cid.clone());
            }
            if let Some(ref cs) = oauth.client_secret {
                provider.params.insert("client_secret".into(), cs.clone());
            }
            if let Some(ref cn) = oauth.client_name {
                provider.params.insert("client_name".into(), cn.clone());
            }
            if let Some(port) = oauth.callback_port {
                provider
                    .params
                    .insert("callback_port".into(), port.to_string());
            }
            if let Some(ref cu) = oauth.callback_url {
                provider.params.insert("callback_url".into(), cu.clone());
            }
            if let Some(ref cr) = oauth.client_registration {
                provider
                    .params
                    .insert("client_registration".into(), cr.clone());
            }
            if let Some(ref amu) = oauth.auth_server_metadata_url {
                provider
                    .params
                    .insert("auth_server_metadata_url".into(), amu.clone());
            }
        }
        let source = Source {
            layer: Layer::Mcp,
            path: Some(path.clone()),
        };
        entries.push(Entry {
            id,
            provider,
            source,
            oauth_config: None,
        });
    }
    (entries, problems)
}

pub fn info_from_entry(entry: &Entry) -> Option<McpProviderInfo> {
    let url = entry.provider.url.clone()?;
    let server_name = entry
        .provider
        .name
        .clone()
        .unwrap_or_else(|| entry.id.clone());
    Some(McpProviderInfo {
        id: entry.id.clone(),
        server_name,
        url,
        scope: entry.provider.params.get("scope").cloned(),
        client_id: entry.provider.params.get("client_id").cloned(),
        client_secret: entry.provider.params.get("client_secret").cloned(),
        client_name: entry.provider.params.get("client_name").cloned(),
        callback_port: entry
            .provider
            .params
            .get("callback_port")
            .and_then(|v| v.parse().ok()),
        callback_url: entry.provider.params.get("callback_url").cloned(),
        client_registration: entry.provider.params.get("client_registration").cloned(),
        auth_server_metadata_url: entry
            .provider
            .params
            .get("auth_server_metadata_url")
            .cloned(),
    })
}
