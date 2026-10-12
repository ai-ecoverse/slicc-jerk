use std::collections::BTreeMap;
use std::collections::BTreeSet;

const IMPLIED_SCOPES: &[(&str, &[&str])] = &[
    (
        "repo",
        &[
            "repo:status",
            "repo_deployment",
            "public_repo",
            "repo:invite",
            "security_events",
        ],
    ),
    ("user", &["read:user", "user:email", "user:follow"]),
    ("project", &["read:project"]),
];

const PRIVILEGE_LADDER: &[(&str, &[&str])] =
    &[("admin:", &["write:", "read:"]), ("write:", &["read:"])];

fn normalize_scopes(raw: &str) -> BTreeSet<String> {
    raw.split(|c: char| c.is_whitespace() || c == ',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn directly_implied(scope: &str) -> Vec<String> {
    let mut implied = Vec::new();

    let map: BTreeMap<&str, &[&str]> = IMPLIED_SCOPES.iter().copied().collect();
    if let Some(children) = map.get(scope) {
        for child in *children {
            implied.push((*child).to_string());
        }
    }

    for &(prefix, targets) in PRIVILEGE_LADDER {
        if let Some(suffix) = scope.strip_prefix(prefix) {
            if !suffix.is_empty() {
                for target in targets {
                    implied.push(format!("{target}{suffix}"));
                }
            }
        }
    }

    implied
}

pub fn expand_scopes(scopes: &BTreeSet<String>) -> BTreeSet<String> {
    let mut expanded = scopes.clone();
    let mut pending: Vec<String> = scopes.iter().cloned().collect();
    while let Some(scope) = pending.pop() {
        for implied in directly_implied(&scope) {
            if expanded.insert(implied.clone()) {
                pending.push(implied);
            }
        }
    }
    expanded
}

pub fn scopes_satisfied(granted: Option<&str>, requested: &str) -> bool {
    let granted_set = match granted {
        Some(g) => normalize_scopes(g),
        None => return false,
    };
    if granted_set.is_empty() {
        return false;
    }
    let requested_set = normalize_scopes(requested);
    if requested_set.is_empty() {
        return true;
    }
    let covered = expand_scopes(&granted_set);
    requested_set.iter().all(|s| covered.contains(s))
}
