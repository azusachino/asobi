use super::*;

#[test]
fn test_normalize_key_basic() {
    assert_eq!(normalize_key("User Preferences"), "User-Preferences");
}

#[test]
fn test_normalize_key_symbols() {
    assert_eq!(normalize_key("Hello@World!"), "HelloWorld");
    assert_eq!(normalize_key("ame:mobile-support:task-1"), "ame:mobile-support:task-1");
    assert_eq!(normalize_key("my.key_with_dots:and_colons"), "my.key_with_dots:and_colons");
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
    // Current implementation preserves multiple hyphens if they are in the middle
    assert_eq!(normalize_key("a---b"), "a---b");
}

#[test]
fn test_normalize_key_unicode() {
    // New implementation drops non-ascii alphanumeric characters that are not in the allowed set
    // and trims trailing non-alphanumeric characters (like the remaining hyphen)
    assert_eq!(normalize_key("node-日本語"), "node");
}

#[test]
fn test_normalize_key_empty() {
    assert_eq!(normalize_key(""), "");
    assert_eq!(normalize_key("---"), "");
}
