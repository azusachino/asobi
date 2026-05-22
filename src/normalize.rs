pub fn normalize_key(key: &str) -> String {
    slug::slugify(key)
}

#[cfg(test)]
#[path = "normalize_test.rs"]
mod tests;
