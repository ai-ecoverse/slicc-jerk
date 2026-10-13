#[derive(Clone, Debug, PartialEq)]
pub struct WwwAuthenticate {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub fn parse_www_authenticate(header: &str) -> WwwAuthenticate {
    let trimmed = header.trim_start();
    let scheme = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if scheme != "bearer" && scheme != "dpop" {
        return WwwAuthenticate {
            resource_metadata: None,
            scope: None,
            error: None,
            error_description: None,
        };
    }
    WwwAuthenticate {
        resource_metadata: field(header, "resource_metadata"),
        scope: field(header, "scope"),
        error: field(header, "error"),
        error_description: field(header, "error_description"),
    }
}

fn field(header: &str, name: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let name_lower = name.to_ascii_lowercase();
    let pat = format!("{}=", name_lower);
    let pos = lower.find(&pat)?;
    let after = &header[pos + name.len() + 1..];
    if let Some(stripped) = after.strip_prefix('"') {
        let end = stripped.find('"')?;
        let val = &stripped[..end];
        if val.is_empty() {
            return None;
        }
        Some(val.to_string())
    } else {
        let end = after
            .find(|c: char| c == ',' || c.is_whitespace())
            .unwrap_or(after.len());
        let val = &after[..end];
        if val.is_empty() {
            return None;
        }
        Some(val.to_string())
    }
}
