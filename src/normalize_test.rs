use super::*;

#[test]
fn test_normalize_key_basic() {
    assert_eq!(normalize_key("User Preferences"), "user-preferences");
}

#[test]
fn test_normalize_key_symbols() {
    assert_eq!(normalize_key("Hello@World!"), "hello-world");
}

#[test]
fn test_normalize_key_trailing_hyphens() {
    assert_eq!(
        normalize_key("--Leading-And-Trailing--"),
        "leading-and-trailing"
    );
}

#[test]
fn test_normalize_key_multiple_hyphens() {
    assert_eq!(normalize_key("a---b"), "a-b");
}

#[test]
fn test_normalize_key_unicode() {
    assert_eq!(normalize_key("node-日本語"), "node-ri-ben-yu");
}

#[test]
fn test_normalize_key_empty() {
    assert_eq!(normalize_key(""), "");
    assert_eq!(normalize_key("---"), "");
}
