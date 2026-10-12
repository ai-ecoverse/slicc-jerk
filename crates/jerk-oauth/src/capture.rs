use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum CaptureError {
    MissingParam(String),
    StateMismatch,
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

pub fn parse_redirect(
    redirect_url: &str,
    from: &crate::config::CaptureFrom,
    param: &str,
    expected_state: Option<&str>,
) -> Result<CaptureResult, CaptureError> {
    let params = extract_params(redirect_url, from);

    if let Some(error) = params.get("error") {
        return Err(CaptureError::IdpError {
            error: error.clone(),
            description: params.get("error_description").cloned(),
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

fn extract_params(url_str: &str, from: &crate::config::CaptureFrom) -> BTreeMap<String, String> {
    match from {
        crate::config::CaptureFrom::Query => parse_query_from_url(url_str),
        crate::config::CaptureFrom::Fragment => parse_fragment_from_url(url_str),
        _ => BTreeMap::new(),
    }
}

fn parse_query_from_url(url_str: &str) -> BTreeMap<String, String> {
    let query = match url_str.find('?') {
        Some(i) => {
            let after = &url_str[i + 1..];
            match after.find('#') {
                Some(j) => &after[..j],
                None => after,
            }
        }
        None => return BTreeMap::new(),
    };
    parse_pairs(query)
}

fn parse_fragment_from_url(url_str: &str) -> BTreeMap<String, String> {
    match url_str.find('#') {
        Some(i) => parse_pairs(&url_str[i + 1..]),
        None => BTreeMap::new(),
    }
}

fn parse_pairs(input: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
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
        map.entry(key).or_insert(value);
    }
    map
}

fn url_decode(input: &str) -> String {
    url::form_urlencoded::parse(input.as_bytes())
        .next()
        .map(|(k, _)| k.into_owned())
        .unwrap_or_else(|| input.to_string())
}
