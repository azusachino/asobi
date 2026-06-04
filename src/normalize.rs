pub fn normalize_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut prev_dash = false;
    for ch in key.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.') {
            out.push(ch);
            prev_dash = false;
        } else if ch.is_whitespace() {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        // else: drop unsafe char
    }
    out.trim_matches('-').to_string()
}

#[cfg(test)]
#[path = "normalize_test.rs"]
mod tests;
