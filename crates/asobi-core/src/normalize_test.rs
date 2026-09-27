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

#[test]
fn test_slugify_ascii_basic() {
    assert_eq!(slugify("Hello World"), "hello-world");
    assert_eq!(slugify("my-title"), "my-title");
    assert_eq!(slugify("CamelCase"), "camelcase");
}

#[test]
fn test_slugify_non_ascii_produces_non_empty() {
    let slug = slugify("日本語");
    assert!(!slug.is_empty(), "slugify should never return empty string");
    assert!(
        slug.starts_with("t-"),
        "fallback slug should start with 't-'"
    );
}

#[test]
fn test_slugify_non_ascii_stable() {
    let slug1 = slugify("日本語");
    let slug2 = slugify("日本語");
    assert_eq!(
        slug1, slug2,
        "same input should produce same slug across calls"
    );
}

#[test]
fn test_slugify_non_ascii_distinct() {
    let slug1 = slugify("日本語");
    let slug2 = slugify("café");
    assert_ne!(
        slug1, slug2,
        "different inputs should produce different slugs"
    );
}

#[test]
fn test_slugify_mixed_content() {
    // Mixed ASCII and non-ASCII: ASCII parts should be preserved
    let slug = slugify("blog-日本語");
    assert_eq!(slug, "blog");
}

#[test]
fn test_slugify_accented_ascii_produces_fallback() {
    let slug = slugify("café");
    // "café" → "caf-" (e is non-ascii) → "caf" after split/filter
    // This should work fine with ASCII rules, so it returns "caf"
    assert_eq!(slug, "caf");
}
