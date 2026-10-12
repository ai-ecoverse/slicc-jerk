use url::form_urlencoded;

pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut result = template.to_string();
    for &(name, value) in vars {
        let placeholder = format!("{{{name}}}");
        let encoded: String = form_urlencoded::byte_serialize(value.as_bytes()).collect();
        result = result.replace(&placeholder, &encoded);
    }
    result
}
