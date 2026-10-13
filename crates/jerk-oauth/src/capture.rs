use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    MissingParam(String),
    StateMismatch,
    RedirectMismatch,
    DuplicateParam(String),
    IdpError {
        error: String,
        description: Option<String>,
    },
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureError::MissingParam(name) => {
                write!(f, "missing parameter: {name}")
            }
            CaptureError::StateMismatch => write!(f, "state mismatch"),
            CaptureError::RedirectMismatch => write!(f, "redirect URI mismatch"),
            CaptureError::DuplicateParam(name) => {
                write!(f, "duplicate parameter: {name}")
            }
            CaptureError::IdpError { error, description } => match description {
                Some(desc) => write!(f, "IdP error: {error}: {desc}"),
                None => write!(f, "IdP error: {error}"),
            },
        }
    }
}

#[derive(Debug)]
pub struct CaptureResult {
    pub value: String,
    pub params: BTreeMap<String, String>,
}

pub fn verify_redirect_uri(
    captured_url: &str,
    configured_redirect: &str,
    actual_port: Option<u16>,
) -> Result<(), CaptureError> {
    let captured = match url::Url::parse(captured_url) {
        Ok(u) => u,
        Err(_) => return Err(CaptureError::RedirectMismatch),
    };
    let configured = match url::Url::parse(configured_redirect) {
        Ok(u) => u,
        Err(_) => return Err(CaptureError::RedirectMismatch),
    };

    if captured.scheme() != configured.scheme() {
        return Err(CaptureError::RedirectMismatch);
    }

    if captured.host_str() != configured.host_str() {
        return Err(CaptureError::RedirectMismatch);
    }

    let configured_port = actual_port.or_else(|| configured.port());
    let captured_port = captured.port();
    if captured_port != configured_port {
        return Err(CaptureError::RedirectMismatch);
    }

    if captured.path() != configured.path() {
        return Err(CaptureError::RedirectMismatch);
    }

    Ok(())
}

pub fn parse_redirect(
    redirect_url: &str,
    from: &crate::config::CaptureFrom,
    param: &str,
    expected_state: Option<&str>,
    redirect_uri: Option<&str>,
    actual_port: Option<u16>,
) -> Result<CaptureResult, CaptureError> {
    if let Some(configured) = redirect_uri {
        verify_redirect_uri(redirect_url, configured, actual_port)?;
    }

    let (params, duplicates) = extract_params_with_duplicates(redirect_url, from);

    for sensitive in &["code", "state"] {
        if duplicates.contains(&sensitive.to_string()) {
            return Err(CaptureError::DuplicateParam(sensitive.to_string()));
        }
    }

    if let Some(error) = params.get("error") {
        let desc = params
            .get("error_description")
            .map(|d| sanitize_error_description(d));
        return Err(CaptureError::IdpError {
            error: sanitize_error_description(error),
            description: desc,
        });
    }

    if let Some(expected) = expected_state {
        let received = params.get("state").map(String::as_str).unwrap_or("");
        if received != expected {
            return Err(CaptureError::StateMismatch);
        }
    }

    let value = params
        .get(param)
        .ok_or_else(|| CaptureError::MissingParam(param.to_string()))?
        .clone();

    Ok(CaptureResult { value, params })
}

fn extract_params_with_duplicates(
    url_str: &str,
    from: &crate::config::CaptureFrom,
) -> (BTreeMap<String, String>, Vec<String>) {
    match from {
        crate::config::CaptureFrom::Query => parse_query_from_url_with_duplicates(url_str),
        crate::config::CaptureFrom::Fragment => parse_fragment_from_url_with_duplicates(url_str),
        _ => (BTreeMap::new(), Vec::new()),
    }
}

fn parse_query_from_url_with_duplicates(url_str: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let query = match url_str.find('?') {
        Some(i) => {
            let after = &url_str[i + 1..];
            match after.find('#') {
                Some(j) => &after[..j],
                None => after,
            }
        }
        None => return (BTreeMap::new(), Vec::new()),
    };
    parse_pairs_with_duplicates(query)
}

fn parse_fragment_from_url_with_duplicates(
    url_str: &str,
) -> (BTreeMap<String, String>, Vec<String>) {
    match url_str.find('#') {
        Some(i) => parse_pairs_with_duplicates(&url_str[i + 1..]),
        None => (BTreeMap::new(), Vec::new()),
    }
}

fn parse_pairs_with_duplicates(input: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let mut map = BTreeMap::new();
    let mut duplicates = Vec::new();
    for pair in input.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.find('=') {
            Some(i) => (&pair[..i], &pair[i + 1..]),
            None => (pair, ""),
        };
        let key = url_decode(key);
        let value = url_decode(value);
        use std::collections::btree_map::Entry;
        match map.entry(key.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(value);
            }
            Entry::Occupied(_) => {
                if !duplicates.contains(&key) {
                    duplicates.push(key);
                }
            }
        }
    }
    (map, duplicates)
}

fn url_decode(input: &str) -> String {
    url::form_urlencoded::parse(input.as_bytes())
        .next()
        .map(|(k, _)| k.into_owned())
        .unwrap_or_else(|| input.to_string())
}

pub fn sanitize_error_description(input: &str) -> String {
    const MAX_LEN: usize = 128;
    input
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(MAX_LEN)
        .collect()
}
