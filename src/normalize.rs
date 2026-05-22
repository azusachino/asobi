pub fn normalize_key(key: &str) -> String {
    key.to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "-")
        .trim_matches('-')
        .to_string()
}

#[cfg(test)]
#[path = "normalize_test.rs"]
mod tests;
