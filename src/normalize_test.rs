use super::*;

#[test]
fn test_normalize_key_basic() {
    assert_eq!(normalize_key("User Preferences"), "User-Preferences");
}

#[test]
fn test_normalize_key_symbols() {
    assert_eq!(normalize_key("Hello@World!"), "HelloWorld");
    assert_eq!(
        normalize_key("ame:mobile-support:task-1"),
        "ame:mobile-support:task-1"
    );
    assert_eq!(
        normalize_key("my.key_with_dots:and_colons"),
        "my.key_with_dots:and_colons"
    );
    assert_eq!(normalize_key("a : b"), "a:b");
    assert_eq!(normalize_key("a - b"), "a-b");
    assert_eq!(normalize_key("a._:b"), "a:b");
}

#[test]
fn test_normalize_key_trailing_hyphens() {
    assert_eq!(
        normalize_key("--Leading-And-Trailing--"),
        "Leading-And-Trailing"
    );
}

#[test]
fn test_normalize_key_multiple_hyphens() {
    // Current implementation collapses connectors
    assert_eq!(normalize_key("a---b"), "a-b");
}

#[test]
fn test_normalize_key_unicode() {
    // New implementation drops non-ascii alphanumeric characters that are not in the allowed set
    // and trims trailing non-alphanumeric characters
    assert_eq!(normalize_key("node-日本語"), "node");
}

#[test]
fn test_normalize_key_edge_cases() {
    assert_eq!(
        normalize_key("ame:test space:task 1"),
        "ame:test-space:task-1"
    );
    assert_eq!(normalize_key("a : b"), "a:b");
    assert_eq!(normalize_key("a - b"), "a-b");
    assert_eq!(normalize_key("a  b"), "a-b");
}
