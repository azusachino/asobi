//! Shared YAML-frontmatter handling for Markdown topics and skills.
//!
//! One place owns the read/write contract so the compact *writer*
//! ([`quote`]) and the skills *readers* ([`parse`]) can never drift:
//! quoting added on write is always reversed on read, the closing `---`
//! must be a whole line (a thematic break in the body never truncates the
//! document), and CRLF endings are tolerated everywhere.
//!
//! Deliberately *not* a real YAML/Markdown library. Topics carry only a flat,
//! three-key frontmatter (`title`/`type`/`slug`) over a free-prose body that
//! gets written into the graph/Markdown projection verbatim — so a full parser buys no
//! correctness and a body escaper would pollute the stored context. We harden just the two
//! edges that actually bite (frontmatter quoting + a non-greedy fence) and keep
//! the body untouched. See decision `asobi:decision:topic-markdown-no-escaper`.
//!
//! Scope of the supported subset: a leading `---` block of `key: value` lines,
//! values optionally wrapped in matching single/double quotes; everything after
//! the closing `---` is the opaque body. No nesting, lists, multi-line scalars,
//! or comments — callers ([`crate::compact`] and [`crate::skills`]) only ever
//! emit/consume that shape.

use std::collections::BTreeMap;

/// A parsed leading YAML frontmatter block: its `key: value` fields and the
/// body that follows the closing `---` fence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    pub fields: BTreeMap<String, String>,
    pub body: String,
}

impl Frontmatter {
    /// The unquoted value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// Quote a value as a YAML double-quoted scalar so names containing `: `, or a
/// leading `#`/`@`/`[`/`!`, stay valid under strict YAML parsers (e.g.
/// Obsidian). Escapes `\` then `"` per the double-quoted spec; [`parse`]
/// reverses it.
pub fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Parse a leading YAML frontmatter block. Returns `None` when the text has no
/// leading `---` line or the fence is never closed. The closing fence must be a
/// whole `---` line — matching that, not a `\n---` substring, keeps a thematic
/// break or dash-rule inside the body from truncating the document. CRLF line
/// endings are tolerated.
pub fn parse(raw: &str) -> Option<Frontmatter> {
    let mut lines = raw.split_inclusive('\n');
    let first = lines.next()?;
    if !is_fence(first) {
        return None;
    }

    let mut fields = BTreeMap::new();
    let mut consumed = first.len();
    let mut closed = false;
    for line in lines {
        consumed += line.len();
        if is_fence(line) {
            closed = true;
            break;
        }
        if let Some((key, value)) = line.trim_end_matches(['\r', '\n']).split_once(':') {
            fields.insert(key.trim().to_string(), unquote(value.trim()));
        }
    }

    if !closed {
        return None;
    }

    let body = raw[consumed..].trim_start_matches(['\r', '\n']).to_string();
    Some(Frontmatter { fields, body })
}

/// A `---` fence line, ignoring trailing CR/LF and surrounding spaces.
fn is_fence(line: &str) -> bool {
    line.trim_end_matches(['\r', '\n']).trim() == "---"
}

/// Reverse [`quote`]: strip a surrounding double- or single-quoted YAML scalar.
/// Double quotes unescape `\"`/`\\`; single quotes unescape `''`. Bare values
/// pass through trimmed, so legacy unquoted frontmatter is untouched.
fn unquote(value: &str) -> String {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        v[1..v.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        v.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_quotes_and_backslashes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn quote_then_parse_roundtrips_colon_names() {
        let name = "asobi:decision:no-pwa";
        let raw = format!("---\ntitle: {}\n---\n\nbody\n", quote(name));
        let fm = parse(&raw).expect("frontmatter parses");
        assert_eq!(fm.get("title"), Some(name));
        assert_eq!(fm.body, "body\n");
    }

    #[test]
    fn parse_unquotes_double_and_single() {
        let raw = "---\nname: \"my-skill\"\ndescription: 'it''s fine'\n---\nbody";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.get("name"), Some("my-skill"));
        assert_eq!(fm.get("description"), Some("it's fine"));
    }

    #[test]
    fn parse_returns_none_without_leading_fence() {
        assert_eq!(parse("no frontmatter here"), None);
    }

    #[test]
    fn parse_returns_none_when_fence_unclosed() {
        assert_eq!(parse("---\ntitle: x\nno closing fence\n"), None);
    }

    #[test]
    fn parse_preserves_body_thematic_break() {
        // A `---` line inside the body must not be read as the closing fence.
        let raw = "---\ntitle: Foo\n---\n\nIntro\n\n---\n\nMore body\n";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.get("title"), Some("Foo"));
        assert!(fm.body.starts_with("Intro"));
        assert!(
            fm.body.contains("---"),
            "thematic break dropped: {}",
            fm.body
        );
        assert!(fm.body.contains("More body"));
    }

    #[test]
    fn parse_tolerates_crlf() {
        let raw = "---\r\ntitle: \"a:b\"\r\n---\r\nbody line\r\n";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.get("title"), Some("a:b"));
        assert_eq!(fm.body, "body line\r\n");
    }

    #[test]
    fn parse_missing_key_is_none_not_empty() {
        let raw = "---\nname: partial\n---\nbody";
        let fm = parse(raw).unwrap();
        assert_eq!(fm.get("name"), Some("partial"));
        assert_eq!(fm.get("description"), None);
    }
}
