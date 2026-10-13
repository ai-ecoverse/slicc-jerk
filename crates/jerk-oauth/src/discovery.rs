use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ProtectedResourceMetadata {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub scopes_supported: Vec<String>,
}

pub fn parse_protected_resource_metadata(json: &str) -> Result<ProtectedResourceMetadata, String> {
    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let resource = map
        .get("resource")
        .and_then(|v| v.as_str())
        .ok_or("missing resource")?
        .to_string();
    let authorization_servers = map
        .get("authorization_servers")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let scopes_supported = map
        .get("scopes_supported")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(ProtectedResourceMetadata {
        resource,
        authorization_servers,
        scopes_supported,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct AuthorizationServerMetadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub revocation_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub client_id_metadata_document_supported: bool,
    pub authorization_response_iss_parameter_supported: bool,
}

pub fn parse_authorization_server_metadata(
    json: &str,
) -> Result<AuthorizationServerMetadata, String> {
    let map: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let issuer = map
        .get("issuer")
        .and_then(|v| v.as_str())
        .ok_or("missing issuer")?
        .to_string();
    let authorization_endpoint = map
        .get("authorization_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("missing authorization_endpoint")?
        .to_string();
    let token_endpoint = map
        .get("token_endpoint")
        .and_then(|v| v.as_str())
        .ok_or("missing token_endpoint")?
        .to_string();
    let registration_endpoint = map
        .get("registration_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from);
    let revocation_endpoint = map
        .get("revocation_endpoint")
        .and_then(|v| v.as_str())
        .map(String::from);
    let scopes_supported = string_array(&map, "scopes_supported");
    let response_types_supported = string_array(&map, "response_types_supported");
    let grant_types_supported = string_array(&map, "grant_types_supported");
    let token_endpoint_auth_methods_supported =
        string_array(&map, "token_endpoint_auth_methods_supported");
    let code_challenge_methods_supported = string_array(&map, "code_challenge_methods_supported");
    let client_id_metadata_document_supported = map
        .get("client_id_metadata_document_supported")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let authorization_response_iss_parameter_supported = map
        .get("authorization_response_iss_parameter_supported")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(AuthorizationServerMetadata {
        issuer,
        authorization_endpoint,
        token_endpoint,
        registration_endpoint,
        revocation_endpoint,
        scopes_supported,
        response_types_supported,
        grant_types_supported,
        token_endpoint_auth_methods_supported,
        code_challenge_methods_supported,
        client_id_metadata_document_supported,
        authorization_response_iss_parameter_supported,
    })
}

fn string_array(map: &BTreeMap<String, serde_json::Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

pub fn validate_issuer(
    metadata: &AuthorizationServerMetadata,
    expected: &str,
) -> Result<(), String> {
    let meta_issuer = metadata
        .issuer
        .strip_suffix('/')
        .unwrap_or(&metadata.issuer);
    let exp = expected.strip_suffix('/').unwrap_or(expected);
    if meta_issuer != exp {
        return Err(format!(
            "issuer mismatch: expected {expected}, got {}",
            metadata.issuer
        ));
    }
    Ok(())
}

pub fn well_known_prm_urls(server_url: &url::Url) -> Vec<url::Url> {
    let path_suffix = if server_url.path().ends_with('/') {
        &server_url.path()[..server_url.path().len() - 1]
    } else {
        server_url.path()
    };
    let mut urls = Vec::new();
    if let Ok(u) = url::Url::parse(&format!(
        "{}/.well-known/oauth-protected-resource{}",
        server_url.origin().unicode_serialization(),
        path_suffix
    )) {
        urls.push(u);
    }
    if !path_suffix.is_empty() {
        if let Ok(u) = url::Url::parse(&format!(
            "{}/.well-known/oauth-protected-resource",
            server_url.origin().unicode_serialization()
        )) {
            urls.push(u);
        }
    }
    urls
}

pub fn as_discovery_urls(issuer_url: &str) -> Vec<(url::Url, &'static str)> {
    let issuer = match url::Url::parse(issuer_url) {
        Ok(u) => u,
        Err(_) => return Vec::new(),
    };
    let path_suffix = if issuer.path().ends_with('/') {
        &issuer.path()[..issuer.path().len() - 1]
    } else {
        issuer.path()
    };
    let origin = issuer.origin().unicode_serialization();
    let mut urls = Vec::new();
    if let Ok(u) = url::Url::parse(&format!(
        "{origin}/.well-known/oauth-authorization-server{path_suffix}"
    )) {
        urls.push((u, "oauth"));
    }
    if let Ok(u) = url::Url::parse(&format!(
        "{origin}/.well-known/openid-configuration{path_suffix}"
    )) {
        urls.push((u, "oidc"));
    }
    if !path_suffix.is_empty() {
        if let Ok(u) = url::Url::parse(&format!(
            "{origin}{path_suffix}/.well-known/openid-configuration"
        )) {
            urls.push((u, "oidc"));
        }
    }
    urls
}

pub fn resource_url(server_url: &str) -> Result<String, String> {
    let mut u = url::Url::parse(server_url).map_err(|e| format!("invalid URL: {e}"))?;
    u.set_fragment(None);
    Ok(u.to_string())
}

pub fn select_resource(
    server_url: &str,
    metadata: &ProtectedResourceMetadata,
) -> Result<String, String> {
    let requested = url::Url::parse(server_url).map_err(|e| format!("invalid URL: {e}"))?;
    let configured =
        url::Url::parse(&metadata.resource).map_err(|e| format!("invalid resource URL: {e}"))?;
    if requested.origin() != configured.origin() {
        return Err(format!(
            "protected resource {} does not match server {}",
            metadata.resource, server_url
        ));
    }
    let req_path = if requested.path().ends_with('/') {
        requested.path().to_string()
    } else {
        format!("{}/", requested.path())
    };
    let cfg_path = if configured.path().ends_with('/') {
        configured.path().to_string()
    } else {
        format!("{}/", configured.path())
    };
    if !req_path.starts_with(&cfg_path) {
        return Err(format!(
            "protected resource {} does not match server {}",
            metadata.resource, server_url
        ));
    }
    Ok(metadata.resource.clone())
}
