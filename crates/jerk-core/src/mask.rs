pub fn mask(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 16 {
        return "…".to_string();
    }
    let head = match token.find('_') {
        Some(i) if (1..=4).contains(&i) => i + 1,
        _ => 4,
    };
    let head: String = chars[..head].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

pub fn mask_words(line: &str) -> String {
    line.split(' ')
        .map(|word| {
            let secretish = word.chars().count() >= 24
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-~+/=".contains(c));
            if secretish {
                mask(word)
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
