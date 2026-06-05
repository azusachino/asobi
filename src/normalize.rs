pub fn normalize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut last_type = 0; // 0: None/Alpha, 1: Connector, 2: Separator (:)

    for ch in key.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_type = 0;
        } else if ch == ':' {
            if last_type != 0 {
                out.pop();
            }
            out.push(':');
            last_type = 2;
        } else if matches!(ch, '-' | '_' | '.') || ch.is_whitespace() {
            let connector = if ch.is_whitespace() { '-' } else { ch };
            if last_type == 0 {
                out.push(connector);
                last_type = 1;
            }
            // If last_type is 1 or 2, we ignore additional connectors
        }
    }
    out.trim_matches(|c| c == '-' || c == ':' || c == '_' || c == '.')
        .to_string()
}

pub fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
#[path = "normalize_test.rs"]
mod tests;
