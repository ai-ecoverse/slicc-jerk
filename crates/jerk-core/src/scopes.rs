use std::collections::BTreeSet;

fn implied_by(scope: &str) -> Vec<String> {
    let mut implied: Vec<String> = match scope {
        "repo" => vec![
            "repo:status",
            "repo_deployment",
            "public_repo",
            "repo:invite",
            "security_events",
        ],
        "user" => vec!["read:user", "user:email", "user:follow"],
        "project" => vec!["read:project"],
        _ => vec![],
    }
    .into_iter()
    .map(String::from)
    .collect();
    for (prefix, targets) in [
        ("admin:", &["write:", "read:"][..]),
        ("write:", &["read:"][..]),
    ] {
        if let Some(suffix) = scope.strip_prefix(prefix).filter(|s| !s.is_empty()) {
            implied.extend(targets.iter().map(|t| format!("{t}{suffix}")));
        }
    }
    implied
}

pub fn normalize(raw: &str) -> BTreeSet<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn satisfied(granted: Option<&str>, requested: &str) -> bool {
    let granted = normalize(granted.unwrap_or(""));
    if granted.is_empty() {
        return false;
    }
    let mut covered = granted.clone();
    let mut pending: Vec<String> = granted.into_iter().collect();
    while let Some(scope) = pending.pop() {
        for implied in implied_by(&scope) {
            if covered.insert(implied.clone()) {
                pending.push(implied);
            }
        }
    }
    normalize(requested).iter().all(|s| covered.contains(s))
}
