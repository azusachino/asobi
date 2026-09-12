//! Advisory checks of entry-point references against the planned installation.
//! References never select files, read their targets, or install another skill.

use crate::skill_resources::{guard_path, normalize_reference};
use crate::skills::{CollectedSkill, SkillsTree, is_markdown, parse_frontmatter};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Only single-backtick tokens, simple inline links, and standalone /skill
/// names in prose. Fenced/indented code and multiword commands are excluded.
fn references(body: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut fenced: Option<(u8, usize)> = None;
    for line in body.lines() {
        let trimmed = line.trim_start();
        let indented = line.starts_with("    ") || line.starts_with('\t');
        let marker = trimmed.as_bytes().first().copied();
        let length = trimmed.bytes().take_while(|b| Some(*b) == marker).count();
        if let Some((opening, minimum)) = fenced {
            if !indented
                && marker == Some(opening)
                && length >= minimum
                && trimmed[length..].trim().is_empty()
            {
                fenced = None;
            }
            continue;
        }
        if indented {
            continue;
        }
        if matches!(marker, Some(b'`' | b'~')) && length >= 3 {
            fenced = marker.map(|m| (m, length));
            continue;
        }
        let bytes = line.as_bytes();
        let mut cursor = 0;
        while cursor < bytes.len() {
            if bytes[cursor] == b'`' {
                let count = bytes[cursor..].iter().take_while(|b| **b == b'`').count();
                let start = cursor + count;
                if let Some(end) = line[start..].find(&"`".repeat(count)) {
                    if count == 1 {
                        found.push(&line[start..start + end]);
                    }
                    cursor = start + end + count;
                    continue;
                }
            } else if bytes[cursor] == b']' && bytes.get(cursor + 1) == Some(&b'(') {
                let start = cursor + 2;
                if let Some(end) = line[start..].find(')') {
                    found.push(&line[start..start + end]);
                    cursor = start + end + 1;
                    continue;
                }
            } else if bytes[cursor] == b'/'
                && (cursor == 0
                    || bytes[cursor - 1].is_ascii_whitespace()
                    || b"(['\"".contains(&bytes[cursor - 1]))
            {
                let start = cursor;
                cursor += 1;
                while cursor < bytes.len()
                    && (bytes[cursor].is_ascii_alphanumeric() || b"-_@".contains(&bytes[cursor]))
                {
                    cursor += 1;
                }
                if cursor > start + 1
                    && (cursor == bytes.len()
                        || bytes[cursor].is_ascii_whitespace()
                        || b"),!?:;'\"".contains(&bytes[cursor])
                        || (bytes[cursor] == b'.'
                            && bytes
                                .get(cursor + 1)
                                .is_none_or(|b| b.is_ascii_whitespace())))
                {
                    found.push(&line[start..cursor]);
                }
                continue;
            }
            cursor += 1;
        }
    }
    found
}

pub(crate) fn warn_unresolved<'a>(
    tree: &SkillsTree,
    desired: &[CollectedSkill],
    resources: impl Iterator<Item = &'a PathBuf>,
) {
    let mut files: BTreeSet<PathBuf> = resources.cloned().collect();
    let mut names = BTreeSet::new();
    for skill in desired {
        names.insert(skill.name.clone());
        names.insert(skill.dir_name.clone());
        files.insert(Path::new(&skill.dir_name).join("SKILL.md"));
        for entry in walkdir::WalkDir::new(&skill.bundle_dir)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file() && is_markdown(e.path()))
        {
            if let Ok(relative) = entry.path().strip_prefix(&skill.bundle_dir) {
                files.insert(Path::new(&skill.dir_name).join(relative));
            }
        }
    }
    // Ordinary local skill directories are preserved by materialization.
    // Read only their guarded entry points, never a referenced external path.
    if let Ok(entries) = std::fs::read_dir(&tree.dir) {
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file = entry.path().join("SKILL.md");
            if name.starts_with('.') || name.contains('@') || guard_path(&file).is_err() {
                continue;
            }
            if let Ok(body) = std::fs::read_to_string(file) {
                names.insert(name);
                if let Some((Some(name), _)) = parse_frontmatter(&body) {
                    names.insert(name);
                }
                for local in walkdir::WalkDir::new(entry.path())
                    .into_iter()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_file() && is_markdown(e.path()))
                {
                    if guard_path(local.path()).is_ok()
                        && let Ok(relative) = local.path().strip_prefix(&tree.dir)
                    {
                        files.insert(relative.to_path_buf());
                    }
                }
            }
        }
    }
    for skill in desired {
        let parent = Path::new(&skill.dir_name);
        for token in references(&skill.body).into_iter().collect::<BTreeSet<_>>() {
            if token.is_empty() || token.chars().any(char::is_whitespace) {
                continue;
            }
            let missing = if let Some(name) = token.strip_prefix('/') {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_@".contains(&b))
                    && !names.contains(name)
            } else {
                let path = token.split('#').next().unwrap_or(token);
                !path.is_empty()
                    && !path.contains([':', '\\', '<', '>', '{', '}'])
                    && Path::new(path).extension().is_some()
                    && normalize_reference(parent, path)
                        .is_none_or(|resolved| !files.contains(&resolved))
            };
            if missing {
                tracing::warn!(
                    "{}/SKILL.md: cannot resolve from planned installation: {}",
                    skill.dir_name,
                    token
                );
            }
        }
    }
}
