//! Explicit, source-owned Markdown companions. References never select files.

use crate::skills::{CollectedSkill, derive_source_slug, is_markdown};
use crate::skills_config::SkillSource;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledResource {
    pub path: PathBuf,
    pub source_path: PathBuf,
    pub source: String,
    pub version: String,
}

pub fn validate_relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.to_string_lossy().contains(['\\', '*', '?', '[', ']'])
    {
        bail!(
            "expected an exact relative path without traversal or wildcards: {}",
            path.display()
        );
    }
    Ok(())
}

pub fn validate_resource_path(path: &Path) -> Result<()> {
    validate_relative(path)?;
    if !is_markdown(path) {
        bail!(
            "shared_markdown accepts only .md or .markdown files: {}",
            path.display()
        );
    }
    if path
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"))
    {
        bail!(
            "shared Markdown must not be a SKILL.md entry point: {}",
            path.display()
        );
    }
    Ok(())
}

/// Check every existing component; canonicalizing alone would accept symlinks
/// inside the root, and a missing final file must not hide a linked parent.
pub fn guard_path(path: &Path) -> Result<()> {
    // macOS exposes its temporary directories through these OS-owned aliases.
    // Resolve only that prefix; source/destination symlinks below it still fail.
    #[cfg(target_os = "macos")]
    let expanded = ["/var", "/tmp"].iter().find_map(|prefix| {
        path.strip_prefix(prefix).ok().and_then(|rest| {
            std::fs::canonicalize(prefix)
                .ok()
                .map(|root| root.join(rest))
        })
    });
    #[cfg(target_os = "macos")]
    let path = expanded.as_deref().unwrap_or(path);
    let mut current = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("symlink is not allowed: {}", current.display())
            }
            Ok(meta) if index + 1 < components.len() && !meta.is_dir() => {
                bail!("path parent is not a directory: {}", current.display())
            }
            Ok(meta) if !meta.is_file() && !meta.is_dir() => {
                bail!("not a regular file or directory: {}", current.display())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn resource_target(source: &str, path: &Path) -> Result<PathBuf> {
    validate_resource_path(path)?;
    let slug = crate::normalize::slugify(&derive_source_slug(source));
    validate_relative(Path::new(&slug))?;
    Ok(Path::new(".shared").join(slug).join(path))
}

fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut result = PathBuf::new();
    for _ in common..from.len() {
        result.push("..");
    }
    for part in &to[common..] {
        result.push(part.as_os_str());
    }
    result.to_string_lossy().replace('\\', "/")
}

pub(crate) fn normalize_reference(parent: &Path, token: &str) -> Option<PathBuf> {
    let path = Path::new(token);
    if path.is_absolute() || token.contains([':', '\\']) {
        return None;
    }
    let mut result = parent.to_path_buf();
    for part in path.components() {
        match part {
            Component::Normal(name) => result.push(name),
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(result)
}

/// Deliberately recognizes inline code and simple inline link destinations.
/// Unsupported mentions of a declared reference fail instead of installing a
/// broken pointer. No Markdown syntax can grant access to another source file.
fn rewrite(
    body: &str,
    source_doc: &Path,
    target_doc: &Path,
    mapping: &BTreeMap<PathBuf, PathBuf>,
) -> Result<String> {
    let parent = source_doc.parent().unwrap_or(Path::new(""));
    let target_parent = target_doc.parent().unwrap_or(Path::new(""));
    let mut spans = Vec::new();
    let bytes = body.as_bytes();
    let mut cursor = 0;
    let mut fenced = false;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            cursor += line.len();
            continue;
        }
        let end = cursor + line.len();
        if fenced {
            cursor = end;
            continue;
        }
        while cursor < end {
            let (start, close) = if bytes[cursor] == b'`' {
                (cursor + 1, b'`')
            } else if bytes[cursor] == b']' && bytes.get(cursor + 1) == Some(&b'(') {
                (cursor + 2, b')')
            } else {
                cursor += 1;
                continue;
            };
            let Some(length) = bytes[start..end].iter().position(|b| *b == close) else {
                cursor += 1;
                continue;
            };
            let stop = start + length;
            let token = &body[start..stop];
            let (path, fragment) = token
                .split_once('#')
                .map_or((token, ""), |(p, _)| (p, &token[p.len()..]));
            if let Some(resolved) = normalize_reference(parent, path)
                && let Some(destination) = mapping.get(&resolved)
            {
                spans.push((
                    start,
                    stop,
                    format!("{}{fragment}", relative(target_parent, destination)),
                ));
            }
            cursor = stop + 1;
        }
    }
    // Normalize path candidates even in unsupported syntax. A reference-style
    // destination such as ./../../references/x.md must not evade validation
    // merely because its spelling differs from the shortest relative path.
    let mut offset = 0;
    for token in body.split_inclusive(|c: char| c.is_whitespace() || "`[]()<>\"',".contains(c)) {
        let candidate =
            token.trim_end_matches(|c: char| c.is_whitespace() || "`[]()<>\"',".contains(c));
        let path = candidate.split('#').next().unwrap_or(candidate);
        if let Some(resolved) = normalize_reference(parent, path)
            && mapping.contains_key(&resolved)
            && !spans
                .iter()
                .any(|(start, stop, _)| offset >= *start && offset + candidate.len() <= *stop)
        {
            bail!(
                "unsupported shared Markdown reference in {}: {}; use inline code or a simple Markdown link",
                source_doc.display(),
                candidate
            );
        }
        offset += token.len();
    }
    for source in mapping.keys() {
        let spelling = relative(parent, source);
        for (offset, _) in body.match_indices(&spelling) {
            // A filename suffix inside an unrelated URL or a longer path is
            // not a reference to this declared source document.
            if offset > 0 && body.as_bytes()[offset - 1].is_ascii_alphanumeric()
                || offset > 0 && matches!(body.as_bytes()[offset - 1], b'/' | b'_' | b'-' | b'.')
            {
                continue;
            }
            if !spans
                .iter()
                .any(|(start, stop, _)| offset >= *start && offset + spelling.len() <= *stop)
            {
                bail!(
                    "unsupported shared Markdown reference in {}: {}; use inline code or a simple Markdown link",
                    source_doc.display(),
                    spelling
                );
            }
        }
    }
    let mut output = body.to_owned();
    for (start, stop, replacement) in spans.into_iter().rev() {
        output.replace_range(start..stop, &replacement);
    }
    Ok(output)
}

pub fn prepare(skills: &mut [CollectedSkill], checkout: &Path, config: &SkillSource) -> Result<()> {
    config.selection()?;
    guard_path(checkout)?;
    let source = skills
        .first()
        .map(|s| s.source.as_str())
        .unwrap_or(&config.url);
    let mapping: BTreeMap<_, _> = config
        .shared_markdown
        .iter()
        .map(|path| Ok((path.clone(), resource_target(source, path)?)))
        .collect::<Result<_>>()?;
    let mut shared = BTreeMap::new();
    for (path, target) in &mapping {
        let file = checkout.join(path);
        guard_path(&file)?;
        if !file.is_file() {
            bail!("shared Markdown file is missing: {}", file.display());
        }
        let body = std::fs::read_to_string(file)?;
        shared.insert(path.clone(), rewrite(&body, path, target, &mapping)?);
    }
    for skill in skills {
        skill.source_config = Some(config.clone());
        skill.shared_markdown = shared.clone();
        if mapping.is_empty() {
            continue;
        }
        let source_dir = skill.bundle_dir.strip_prefix(checkout)?;
        let target = Path::new(&skill.dir_name);
        skill.body = rewrite(
            &skill.body,
            &source_dir.join("SKILL.md"),
            &target.join("SKILL.md"),
            &mapping,
        )?;
        for entry in walkdir::WalkDir::new(&skill.bundle_dir) {
            let entry = entry?;
            if !entry.file_type().is_file() || !is_markdown(entry.path()) {
                continue;
            }
            let local = entry.path().strip_prefix(&skill.bundle_dir)?;
            if local == Path::new("SKILL.md") {
                continue;
            }
            guard_path(entry.path())?;
            let body = std::fs::read_to_string(entry.path())?;
            skill.bundle_rewrites.insert(
                local.to_path_buf(),
                rewrite(
                    &body,
                    &source_dir.join(local),
                    &target.join(local),
                    &mapping,
                )?,
            );
        }
    }
    Ok(())
}

pub fn reload(
    dir: &Path,
    source: &str,
    config: Option<&SkillSource>,
) -> Result<BTreeMap<PathBuf, String>> {
    let mut files = BTreeMap::new();
    if let Some(config) = config {
        for path in &config.shared_markdown {
            let file = dir.join(resource_target(source, path)?);
            guard_path(&file)?;
            files.insert(
                path.clone(),
                std::fs::read_to_string(&file).map_err(|e| {
                    anyhow::anyhow!("read installed resource {}: {e}", file.display())
                })?,
            );
        }
    }
    Ok(files)
}

pub type ResourcePlan = BTreeMap<PathBuf, (InstalledResource, String)>;

pub fn plan(
    dir: &Path,
    desired: &[CollectedSkill],
    previous: &[InstalledResource],
) -> Result<ResourcePlan> {
    let mut plan: ResourcePlan = BTreeMap::new();
    let mut namespaces = BTreeMap::new();
    for skill in desired {
        let namespace = crate::normalize::slugify(&derive_source_slug(&skill.source));
        if let Some(previous_source) = namespaces.insert(namespace.clone(), &skill.source)
            && previous_source != &skill.source
        {
            bail!("different sources share namespace {namespace}; use distinct source names");
        }
        for (path, body) in &skill.shared_markdown {
            let target = resource_target(&skill.source, path)?;
            if let Some((existing, content)) = plan.get(&target)
                && (existing.source != skill.source
                    || content != body
                    || existing.version != skill.version)
            {
                bail!("conflicting shared Markdown source at {}", target.display());
            }
            let record = InstalledResource {
                path: target.clone(),
                source_path: path.clone(),
                source: skill.source.clone(),
                version: skill.version.clone(),
            };
            plan.insert(target, (record, body.clone()));
        }
    }
    for target in plan.keys() {
        if plan
            .keys()
            .any(|other| other != target && target.starts_with(other))
        {
            bail!(
                "shared Markdown file/parent collision: {}",
                target.display()
            );
        }
    }
    for record in previous {
        if record.path != resource_target(&record.source, &record.source_path)? {
            bail!(
                "invalid shared resource ownership record: {}",
                record.path.display()
            );
        }
        guard_path(&dir.join(&record.path))?;
        if dir.join(&record.path).is_dir() {
            bail!(
                "owned shared Markdown path is a directory: {}",
                record.path.display()
            );
        }
    }
    for target in plan.keys() {
        let file = dir.join(target);
        guard_path(&file)?;
        if file.exists() && !previous.iter().any(|old| &old.path == target) {
            bail!(
                "shared Markdown destination already exists without ownership: {}",
                file.display()
            );
        }
        if file.is_dir() {
            bail!(
                "shared Markdown destination is a directory: {}",
                file.display()
            );
        }
    }
    Ok(plan)
}

pub fn write(
    dir: &Path,
    plan: ResourcePlan,
    previous: &[InstalledResource],
) -> Result<Vec<InstalledResource>> {
    for (target, (_, body)) in &plan {
        let file = dir.join(target);
        guard_path(&file)?;
        std::fs::create_dir_all(file.parent().unwrap())?;
        if !std::fs::read_to_string(&file).is_ok_and(|old| old == *body) {
            std::fs::write(file, body)?;
        }
    }
    for old in previous {
        if !plan.contains_key(&old.path) {
            let file = dir.join(&old.path);
            guard_path(&file)?;
            match std::fs::remove_file(file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(plan.into_values().map(|(record, _)| record).collect())
}
