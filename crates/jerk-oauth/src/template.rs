use url::form_urlencoded;

const BUILTIN_VARS: &[&str] = &[
    "client_id",
    "redirect_uri",
    "scope",
    "state",
    "nonce",
    "code_challenge",
];

pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for &(name, value) in vars {
        let placeholder = format!("{{{name}}}");
        let encoded: String = form_urlencoded::byte_serialize(value.as_bytes()).collect();
        result = result.replace(&placeholder, &encoded);
    }
    result
}

pub fn extra_placeholders(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else {
            break;
        };
        let name = &rest[..end];
        rest = &rest[end + 1..];
        if name.is_empty() || BUILTIN_VARS.contains(&name) {
            continue;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}
