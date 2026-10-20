use crate::paths::{Env, Os};
use std::path::PathBuf;

pub fn expand(text: &str, env: &Env) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let Some(len) = closing(&rest[start + 2..]) else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &rest[start + 2..start + 2 + len];
        let (name, default) = match inner.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (inner, None),
        };
        match env.var(name) {
            Some(value) => out.push_str(value),
            None => out.push_str(&expand(default.unwrap_or(""), env)),
        }
        rest = &rest[start + len + 3..];
    }
    out.push_str(rest);
    out
}

pub fn closing(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return Some(i),
            '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

pub fn path(text: &str, env: &Env) -> PathBuf {
    let expanded = expand(text, env);
    let home = if env.os == Os::Windows {
        "USERPROFILE"
    } else {
        "HOME"
    };
    match expanded.strip_prefix("~/").or(expanded.strip_prefix("~\\")) {
        Some(rest) => PathBuf::from(env.var(home).unwrap_or("/")).join(rest),
        None if expanded == "~" => PathBuf::from(env.var(home).unwrap_or("/")),
        None => PathBuf::from(expanded),
    }
}

pub fn ini_value(text: &str, key: &str, env: &Env) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with(';') && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .filter(|(k, _)| k.trim() == key)
        .map(|(_, v)| {
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(v);
            expand(v, env)
        })
        .next_back()
        .filter(|v| !v.is_empty())
}
