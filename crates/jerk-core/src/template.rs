use crate::config::placeholders;
use std::collections::BTreeMap;

pub type Vars = BTreeMap<String, String>;

pub fn render_text(text: &str, vars: &Vars) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let dollar = start > 0 && rest.as_bytes()[start - 1] == b'$';
        let close = if dollar {
            crate::source::closing(&rest[start + 1..])
        } else {
            rest[start..].find('}')
        };
        let end = close
            .map(|e| start + e + usize::from(dollar))
            .unwrap_or(rest.len() - 1);
        out.push_str(&rest[..start]);
        let name = &rest[start + 1..end];
        if dollar {
            out.push_str(&rest[start..=end]);
        } else {
            out.push_str(vars.get(name).map(String::as_str).unwrap_or(""));
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn render_args(args: &[String], vars: &Vars) -> Vec<String> {
    let mut out: Vec<(String, bool)> = Vec::new();
    for arg in args {
        let names = placeholders(arg).unwrap_or_default();
        let rendered = render_text(arg, vars);
        let empty = names
            .iter()
            .any(|n| vars.get(n).is_none_or(|v| v.is_empty()));
        if !names.is_empty() && empty {
            if let Some((last, templated)) = out.last() {
                if !templated && last.starts_with('-') {
                    out.pop();
                }
            }
            continue;
        }
        out.push((rendered, !names.is_empty()));
    }
    out.into_iter().map(|(arg, _)| arg).collect()
}
