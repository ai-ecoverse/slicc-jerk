use crate::config::Rewrite;

pub fn apply_rewrites(url: &str, rewrites: &[Rewrite]) -> String {
    let mut current = url.to_string();
    for rule in rewrites {
        if !current.contains(&rule.match_pattern) {
            continue;
        }
        if let Some(replace) = &rule.replace_url {
            current = replace.clone();
            continue;
        }
        if let (Some(append), Ok(mut parsed)) = (&rule.append, url::Url::parse(&current)) {
            for (k, v) in append {
                parsed.query_pairs_mut().append_pair(k, v);
            }
            current = parsed.to_string();
        }
    }
    current
}
