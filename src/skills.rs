use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionMode {
    All,
    Select(Vec<String>),
    Interactive,
}

/// One skill read out of a source checkout, ready to be written to disk.
///
/// Skills live on the filesystem and nowhere else. They were mirrored into the
/// graph as well until 0.7, which meant every skill existed twice with the disk
/// copy as the one agents actually read — and the graph copy accumulating
/// nothing, since a skill has no observations and only ever carried its
/// `description`. The files are the store of record; `.agents/skills/` is a
/// directory the wider Agent Skills ecosystem already understands, and `rg`
/// searches it better than a graph read did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectedSkill {
    /// On-disk directory name: `<source-slug>@<skill-name>`.
    pub dir_name: String,
    /// The frontmatter `name`, as declared.
    pub name: String,
    pub description: String,
    /// Canonical source URL or path this came from.
    pub source: String,
    /// Resolved git commit, or `local` for a path source.
    pub version: String,
    /// The full `SKILL.md` text, frontmatter included.
    pub body: String,
}

/// What one sync changed on disk.
#[derive(Debug, Default)]
pub struct MaterializeOutcome {
    /// Skill directories created or rewritten.
    pub written: Vec<String>,
    /// Skill directories removed because the config no longer declares them.
    pub removed: Vec<String>,
}

pub fn parse_frontmatter(content: &str) -> Option<(Option<String>, Option<String>)> {
    let fm = crate::frontmatter::parse(content)?;
    Some((
        fm.get("name").map(str::to_string),
        fm.get("description").map(str::to_string),
    ))
}

pub fn derive_source_slug(url: &str) -> String {
    let mut trimmed = url.trim();
    if trimmed.ends_with(".git") {
        trimmed = &trimmed[..trimmed.len() - 4];
    }

    // Trim trailing slashes
    trimmed = trimmed.trim_end_matches('/');

    // 1. https/http URLs
    if let Some(pos) = trimmed.find("://") {
        let path = &trimmed[pos + 3..];
        if let Some(slash_pos) = path.find('/') {
            let parts: Vec<&str> = path[slash_pos + 1..].split('/').collect();
            if parts.len() >= 2 {
                return format!("{}-{}", parts[0], parts[1]);
            }
        }
    }

    // 2. SSH URL git@github.com:owner/repo
    if let Some(colon_pos) = trimmed.find(':') {
        let path = &trimmed[colon_pos + 1..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 {
            return format!("{}-{}", parts[0], parts[1]);
        }
    }

    // 3. Fallback: take last two components
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() >= 2 {
        let len = parts.len();
        return format!("{}-{}", parts[len - 2], parts[len - 1]);
    }

    crate::normalize::normalize_key(url)
}

/// When a skill file has no frontmatter `name:`, derive it from the filename
/// stem — except for convention filenames (`SKILL.md`, `index.md`), where the
/// skill's identity is the parent directory name.
fn resolve_skill_name_fallback(path: &Path) -> String {
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if file_stem.eq_ignore_ascii_case("SKILL") || file_stem.eq_ignore_ascii_case("index") {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or(file_stem)
            .to_string()
    } else {
        file_stem.to_string()
    }
}

/// How many link hops [`inline_local_references`] follows before giving up.
/// Real skills reference at most one or two levels deep (`SKILL.md` ->
/// `reference.md`); this just bounds a pathological reference chain.
const MAX_REFERENCE_DEPTH: usize = 4;

/// Targets of every `[text](target)` inline link in `content`, in source
/// order and with duplicates kept (the caller dedupes via `visited`).
///
/// A hand-rolled scan rather than a markdown parser or a `regex` dependency:
/// skills only ever link with the plain inline form, and this only needs
/// candidate paths, not a rendered document. Reference-style links
/// (`[text][id]`) are deliberately not matched — rare in skills, and not
/// worth a second lookup table for.
fn markdown_link_targets(content: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    for (i, c) in content.char_indices() {
        if c != '[' {
            continue;
        }
        let Some(close_bracket) = content[i..].find(']') else {
            continue;
        };
        let after_bracket = i + close_bracket + 1;
        if !content[after_bracket..].starts_with('(') {
            continue;
        }
        let paren_start = after_bracket + 1;
        let Some(close_paren) = content[paren_start..].find(')') else {
            continue;
        };
        // A link target may carry a trailing `"title"`; only the path matters.
        let raw = content[paren_start..paren_start + close_paren].trim();
        let target = raw.split_whitespace().next().unwrap_or(raw);
        if !target.is_empty() {
            targets.push(target);
        }
    }
    targets
}

/// Inline-code-span targets in `content` that look like a path to a markdown
/// file: single backticks, no whitespace inside, ending in `.md`/`.markdown`.
/// Skips spans inside fenced (```` ``` ````) code blocks, since those are
/// examples to show the reader, not live references.
///
/// Some skills — Anthropic's own `skill-creator` among them — mention sibling
/// docs as prose ("see `references/schemas.md` for the schema") rather than
/// as a markdown link. The ending-in-`.md` + no-whitespace filter is loose by
/// itself (it would also match a *generated output* mentioned in backticks,
/// like "produces `benchmark.md`"), so the real filter is downstream: the
/// caller only inlines a target that resolves to a file that actually exists
/// in the source checkout, which a generated-output mention never does.
fn backtick_path_targets(content: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Splitting on the delimiter puts every span *between* a pair of
        // backticks at an odd index; unmatched trailing backticks just leave
        // a final segment that is never treated as "inside".
        for (idx, span) in line.split('`').enumerate() {
            if idx % 2 == 0 {
                continue;
            }
            let looks_like_markdown_path = span.ends_with(".md") || span.ends_with(".markdown");
            if !span.is_empty() && !span.contains(char::is_whitespace) && looks_like_markdown_path {
                targets.push(span);
            }
        }
    }
    targets
}

/// True for a link target worth following on disk: not an external URL, mail
/// link, or a same-page anchor.
fn is_local_reference(target: &str) -> bool {
    !target.starts_with('#')
        && !target.contains("://")
        && !target.starts_with("mailto:")
        && !target.starts_with("tel:")
}

/// Inline the local `.md`/`.markdown` files a skill references — via a
/// markdown link or a backtick-quoted path — so a `SKILL.md` that is itself
/// just a table of contents over sibling docs ships as one self-contained
/// body. Asobi stores and materializes a single string per skill (see
/// [`materialize_skills`]), so anything the skill needs at runtime has to
/// live in that string — a reference file left beside `SKILL.md` in the
/// source repo never reaches `.agents/skills/<slug>@<name>/`.
///
/// Only references that resolve inside `root_dir` are followed (no escaping
/// the source checkout via `../..`), and a reference to another skill's own
/// entry point (`SKILL.md`/`index.md`) is never inlined — that is a
/// cross-skill reference to something installed as its own entity, not local
/// content. `depth` bounds how many reference hops are followed; `visited`
/// prevents cycles and re-inlining the same file reached two different ways.
fn inline_local_references(
    content: &str,
    file_dir: &Path,
    root_dir: &Path,
    visited: &mut std::collections::HashSet<std::path::PathBuf>,
    depth: usize,
) -> String {
    if depth == 0 {
        return content.to_string();
    }
    let mut out = content.to_string();
    let mut candidates = markdown_link_targets(content);
    candidates.extend(backtick_path_targets(content));
    for target in candidates {
        if !is_local_reference(target) {
            continue;
        }
        // Strip a `#fragment` before resolving; the file is what gets inlined.
        let relative = target.split('#').next().unwrap_or(target);
        let Ok(resolved) = file_dir.join(relative).canonicalize() else {
            continue;
        };
        if !resolved.starts_with(root_dir) {
            continue;
        }
        let is_markdown = resolved
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| {
                ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
            });
        let is_entry_point = resolved
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| {
                stem.eq_ignore_ascii_case("SKILL") || stem.eq_ignore_ascii_case("index")
            });
        if !is_markdown || is_entry_point || !visited.insert(resolved.clone()) {
            continue;
        }
        let Ok(ref_content) = std::fs::read_to_string(&resolved) else {
            continue;
        };
        let label = resolved
            .strip_prefix(root_dir)
            .unwrap_or(&resolved)
            .display();
        out.push_str(&format!("\n\n---\n\n<!-- inlined: {label} -->\n\n"));
        let ref_dir = resolved.parent().unwrap_or(file_dir).to_path_buf();
        out.push_str(&inline_local_references(
            &ref_content,
            &ref_dir,
            root_dir,
            visited,
            depth - 1,
        ));
    }
    out
}

/// Entry point for [`inline_local_references`]: canonicalizes `root_dir` once
/// and seeds `visited` with the skill file's own path so a link back to
/// itself is a no-op rather than a duplicate of the whole body.
fn inline_references(content: &str, file_path: &Path, root_dir: &Path) -> String {
    let Some(file_dir) = file_path.parent() else {
        return content.to_string();
    };
    let Ok(root) = root_dir.canonicalize() else {
        return content.to_string();
    };
    let mut visited = std::collections::HashSet::new();
    if let Ok(canonical_self) = file_path.canonicalize() {
        visited.insert(canonical_self);
    }
    inline_local_references(content, file_dir, &root, &mut visited, MAX_REFERENCE_DEPTH)
}

pub fn resolve_selection(
    skills: &[(String, String)],
    mode: SelectionMode,
    is_tty: bool,
) -> Result<Vec<String>> {
    match mode {
        SelectionMode::All => Ok(skills.iter().map(|(n, _)| n.clone()).collect()),
        SelectionMode::Select(names) => {
            let mut selected = Vec::new();
            for name in names {
                if skills.iter().any(|(n, _)| n == &name) {
                    selected.push(name);
                } else {
                    bail!("Skill '{}' not found in source", name);
                }
            }
            Ok(selected)
        }
        SelectionMode::Interactive => {
            if !is_tty {
                bail!("Cannot resolve selection interactively: not a TTY. Use --all or --select");
            }
            use std::io::{self, Write};
            println!("Available skills:");
            for (i, (name, desc)) in skills.iter().enumerate() {
                println!("  [{}] {} - {}", i + 1, name, desc);
            }
            print!("Enter the numbers of the skills to install (comma-separated, e.g. 1, 3): ");
            io::stdout().flush()?;
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let mut selected = Vec::new();
            for part in input.split(|c: char| c == ',' || c.is_whitespace()) {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Ok(idx) = part.parse::<usize>() {
                    if idx > 0 && idx <= skills.len() {
                        selected.push(skills[idx - 1].0.clone());
                    } else {
                        bail!("Invalid skill index: {}", idx);
                    }
                } else {
                    bail!("Invalid input: {}", part);
                }
            }
            if selected.is_empty() {
                bail!("No skills selected");
            }
            Ok(selected)
        }
    }
}

/// Read every skill under `dir_path`, apply `mode`, and return what should be
/// written to disk. Touches no storage: the caller hands the result to
/// [`materialize_skills`], which owns both writing and pruning.
pub fn collect_skills_from_dir(
    dir_path: &Path,
    source: &str,
    version: &str,
    mode: SelectionMode,
    is_tty: bool,
) -> Result<Vec<CollectedSkill>> {
    let mut parsed_skills = Vec::new();
    let mut skill_contents = HashMap::new();
    // Every file path that claimed each name. Real-world skill repos sometimes
    // mirror the same skill under two tool-specific directories (e.g.
    // `.opencode/skills/x/` and `skills/x/`) and those mirrors can genuinely
    // diverge in content (different description, different frontmatter), so a
    // `HashMap<String, String>` silently keeping whichever file was walked
    // last -- and resolve_selection still returning the name twice, so the
    // second `.remove()` further down hit an already-emptied slot -- is not
    // safe. Collected here rather than checked eagerly, so a repo-wide name
    // collision only blocks installing *that* name: a narrow `--select` of an
    // unrelated, unambiguous skill in the same source still succeeds.
    let mut skill_paths: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let content = std::fs::read_to_string(entry.path())?.replace("\r\n", "\n");
        if let Some((parsed_name, parsed_desc)) = parse_frontmatter(&content) {
            let name = parsed_name.unwrap_or_else(|| resolve_skill_name_fallback(entry.path()));
            skill_paths
                .entry(name.clone())
                .or_default()
                .push(entry.path().to_path_buf());
            let body = inline_references(&content, entry.path(), dir_path);
            parsed_skills.push((name.clone(), parsed_desc.unwrap_or_default()));
            skill_contents.insert(name, body);
        }
    }
    if parsed_skills.is_empty() {
        bail!("No valid skills found in {}", source);
    }
    let selected_names = resolve_selection(&parsed_skills, mode, is_tty)?;
    if let Some(name) = selected_names
        .iter()
        .find(|n| skill_paths.get(*n).is_some_and(|paths| paths.len() > 1))
    {
        let paths = skill_paths[name]
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        bail!(
            "skill name '{}' is declared in more than one file in {}: {} -- \
             rename one so names stay unique",
            name,
            source,
            paths
        );
    }
    let slug = derive_source_slug(source);
    let mut collected = Vec::new();
    for name in selected_names {
        let body = skill_contents
            .remove(&name)
            .ok_or_else(|| anyhow!("Content missing for skill {}", name))?;
        let description = parsed_skills
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, d)| d.clone())
            .unwrap_or_default();
        collected.push(CollectedSkill {
            dir_name: format!(
                "{}@{}",
                crate::normalize::slugify(&slug),
                crate::normalize::slugify(&name)
            ),
            name,
            description,
            source: source.to_string(),
            version: version.to_string(),
            body,
        });
    }
    Ok(collected)
}

/// A skill as recorded on disk, for `skills` and `skills show`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    /// On-disk directory name: `<source-slug>@<skill-name>`.
    pub dir: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Canonical source URL or path.
    #[serde(default)]
    pub source: String,
    /// The git commit this was taken from, or `local`. Empty when the manifest
    /// was missing and this entry was recovered by scanning.
    #[serde(default)]
    pub version: String,
}

/// Provenance sidecar written beside the installed skills.
///
/// Until 0.7 a skill's source and version lived on its graph entity, so with
/// the graph copy gone they need a home on disk — and the previous arrangement
/// recorded them so poorly that every installed skill in practice carried only
/// a `description` and no version at all. Keeping them here makes `skills`
/// report what commit each skill came from, lets `update` find its sources
/// again, and gives pruning an explicit record instead of inferring intent
/// from directory names.
pub const MANIFEST_FILE: &str = ".asobi-skills.json";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Manifest {
    skills: Vec<InstalledSkill>,
}

/// Read the installed-skill manifest from `dir`.
///
/// Falls back to scanning `<slug>@<name>` directories when the manifest is
/// absent — an older tree, or one a human edited — so listing still works, at
/// the cost of empty `source`/`version`. Directories without `@` are ignored
/// either way, the same convention [`materialize_skills`] prunes by.
pub fn read_installed_skills(dir: &Path) -> Result<Vec<InstalledSkill>> {
    if let Ok(raw) = std::fs::read_to_string(dir.join(MANIFEST_FILE))
        && let Ok(manifest) = serde_json::from_str::<Manifest>(&raw)
    {
        let mut skills = manifest.skills;
        skills.retain(|s| dir.join(&s.dir).join("SKILL.md").is_file());
        skills.sort_by(|a, b| a.dir.cmp(&b.dir));
        return Ok(skills);
    }

    let mut found = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(found),
        Err(e) => return Err(anyhow!("read {}: {e}", dir.display())),
    };
    for entry in entries {
        let entry = entry?;
        let Some(dir_name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !entry.file_type()?.is_dir() || !dir_name.contains('@') {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path().join("SKILL.md")) else {
            continue;
        };
        let (name, description) = parse_frontmatter(&content).unwrap_or((None, None));
        found.push(InstalledSkill {
            name: name.unwrap_or_else(|| {
                dir_name
                    .split_once('@')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| dir_name.clone())
            }),
            description: description.unwrap_or_default(),
            dir: dir_name,
            source: String::new(),
            version: String::new(),
        });
    }
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(found)
}

/// Write `desired` out as `<dir>/<slug>@<name>/SKILL.md`, then remove every
/// other `<slug>@<name>` directory under `dir`.
///
/// Pruning is deliberately scoped to the `@` naming convention: directories
/// Asobi did not write — a hand-authored skill, a vendored upstream checkout —
/// have no `@` in their name and are never touched. Since 0.7 this is the only
/// place a skill is stored, so this pruning pass is also what retires a skill
/// the config stopped declaring or a source dropped upstream.
pub fn materialize_skills(dir: &Path, desired: &[CollectedSkill]) -> Result<MaterializeOutcome> {
    let mut outcome = MaterializeOutcome::default();
    let wanted: std::collections::HashSet<&str> =
        desired.iter().map(|s| s.dir_name.as_str()).collect();

    std::fs::create_dir_all(dir)?;

    for skill in desired {
        let dir_name = &skill.dir_name;
        let skill_dir = dir.join(dir_name);
        let file = skill_dir.join("SKILL.md");
        // Leave an already-current file alone so a no-op sync does not churn
        // mtimes that file watchers key off.
        if std::fs::read_to_string(&file).is_ok_and(|existing| existing == skill.body) {
            continue;
        }
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(&file, &skill.body)?;
        outcome.written.push(dir_name.clone());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.contains('@') && !wanted.contains(name.as_str()) {
            std::fs::remove_dir_all(entry.path())?;
            outcome.removed.push(name);
        }
    }

    let manifest = Manifest {
        skills: desired
            .iter()
            .map(|s| InstalledSkill {
                dir: s.dir_name.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                source: s.source.clone(),
                version: s.version.clone(),
            })
            .collect(),
    };
    std::fs::write(
        dir.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    outcome.written.sort();
    outcome.removed.sort();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_valid() {
        let content = "---\nname: my-skill\ndescription: \"does something\"\n---\nbody content";
        let parsed = parse_frontmatter(content);
        assert_eq!(
            parsed,
            Some((
                Some("my-skill".to_string()),
                Some("does something".to_string())
            ))
        );
    }

    #[test]
    fn test_parse_frontmatter_missing() {
        let content = "no frontmatter here";
        let parsed = parse_frontmatter(content);
        assert_eq!(parsed, None);
    }

    #[test]
    fn test_parse_frontmatter_malformed() {
        let content = "---\nname: partial-skill\n---\nbody content";
        let parsed = parse_frontmatter(content);
        assert_eq!(parsed, Some((Some("partial-skill".to_string()), None)));
    }

    #[test]
    fn test_derive_source_slug() {
        assert_eq!(
            derive_source_slug("https://github.com/jasonswett/llm-skills.git"),
            "jasonswett-llm-skills"
        );
        assert_eq!(
            derive_source_slug("git@github.com:jasonswett/llm-skills.git"),
            "jasonswett-llm-skills"
        );
        assert_eq!(
            derive_source_slug("/path/to/local-skills"),
            "to-local-skills"
        );
    }

    #[test]
    fn test_resolve_selection_all() {
        let skills = vec![
            ("skill-a".to_string(), "desc-a".to_string()),
            ("skill-b".to_string(), "desc-b".to_string()),
        ];
        let selected = resolve_selection(&skills, SelectionMode::All, false).unwrap();
        assert_eq!(selected, vec!["skill-a", "skill-b"]);
    }

    #[test]
    fn test_resolve_selection_select() {
        let skills = vec![
            ("skill-a".to_string(), "desc-a".to_string()),
            ("skill-b".to_string(), "desc-b".to_string()),
        ];
        let selected = resolve_selection(
            &skills,
            SelectionMode::Select(vec!["skill-b".to_string()]),
            false,
        )
        .unwrap();
        assert_eq!(selected, vec!["skill-b"]);
    }

    #[test]
    fn test_install_from_local_git_repo() {
        use tempfile::tempdir;
        let git_dir = tempdir().unwrap();
        let repo_path = git_dir.path();

        // 1. Initialize git repo
        std::process::Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // Set git config for local commit
        std::process::Command::new("git")
            .arg("config")
            .arg("user.name")
            .arg("Test User")
            .current_dir(repo_path)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("config")
            .arg("user.email")
            .arg("test@example.com")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // 2. Create a skill file
        let skill_file = repo_path.join("test-skill.md");
        std::fs::write(
            &skill_file,
            "---\nname: repo-skill\ndescription: cloned skill\n---\nbody text\n",
        )
        .unwrap();

        // 3. Commit the file
        std::process::Command::new("git")
            .arg("add")
            .arg("test-skill.md")
            .current_dir(repo_path)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("initial commit")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // Get HEAD commit hash
        let output = std::process::Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(repo_path)
            .output()
            .unwrap();
        let head_commit = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // 5. Clone and install
        let clone_temp_dir = tempdir().unwrap();
        let clone_path = clone_temp_dir.path();
        std::process::Command::new("git")
            .arg("clone")
            .arg(repo_path.to_str().unwrap())
            .arg(clone_path.to_str().unwrap())
            .status()
            .unwrap();

        let collected = collect_skills_from_dir(
            clone_path,
            repo_path.to_str().unwrap(),
            &head_commit,
            SelectionMode::All,
            false,
        )
        .unwrap();

        // 6. Verify what was collected
        assert_eq!(collected.len(), 1);
        assert_eq!(collected[0].name, "repo-skill");
        assert_eq!(collected[0].version, head_commit);
        assert_eq!(
            collected[0].dir_name,
            format!(
                "{}@repo-skill",
                crate::normalize::slugify(&derive_source_slug(repo_path.to_str().unwrap()))
            )
        );
    }

    /// Pruning lives entirely in `materialize_skills` now that disk is the only
    /// store: whatever the caller does not pass in this run is what goes.
    #[test]
    fn test_materialize_prunes_skills_dropped_upstream() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        std::fs::write(
            src.join("alpha.md"),
            "---\nname: alpha\ndescription: a\n---\nalpha body\n",
        )
        .unwrap();
        std::fs::write(
            src.join("beta.md"),
            "---\nname: beta\ndescription: b\n---\nbeta body\n",
        )
        .unwrap();
        let source = src.to_str().unwrap();
        let out_dir = tempdir().unwrap();
        let out = out_dir.path();

        let first = collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap();
        assert_eq!(first.len(), 2);
        materialize_skills(out, &first).unwrap();
        assert_eq!(read_installed_skills(out).unwrap().len(), 2);

        // Upstream removes `beta`; the next sync must drop it from disk.
        std::fs::remove_file(src.join("beta.md")).unwrap();
        let second = collect_skills_from_dir(src, source, "v2", SelectionMode::All, false).unwrap();
        let outcome = materialize_skills(out, &second).unwrap();

        assert_eq!(outcome.removed.len(), 1);
        let installed = read_installed_skills(out).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "alpha");
        assert_eq!(installed[0].version, "v2");
    }

    /// The manifest is what carries source and version, which the graph used to
    /// hold — and held so poorly that no installed skill had a version at all.
    #[test]
    fn test_manifest_records_provenance() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        std::fs::write(
            src.join("alpha.md"),
            "---\nname: alpha\ndescription: a\n---\nalpha body\n",
        )
        .unwrap();
        let out_dir = tempdir().unwrap();
        let out = out_dir.path();
        let collected = collect_skills_from_dir(
            src,
            "https://example.com/o/r.git",
            "abc123",
            SelectionMode::All,
            false,
        )
        .unwrap();
        materialize_skills(out, &collected).unwrap();

        assert!(out.join(MANIFEST_FILE).is_file());
        let installed = read_installed_skills(out).unwrap();
        assert_eq!(installed[0].source, "https://example.com/o/r.git");
        assert_eq!(installed[0].version, "abc123");
        assert_eq!(installed[0].description, "a");
    }

    /// Without a manifest — an older tree, or one a human pruned by hand —
    /// listing still works off the directory names, minus the provenance.
    #[test]
    fn test_read_installed_falls_back_to_scanning() {
        use tempfile::tempdir;
        let out_dir = tempdir().unwrap();
        let out = out_dir.path();
        let dir = out.join("some-source@alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: alpha\ndescription: a\n---\nbody\n",
        )
        .unwrap();

        let installed = read_installed_skills(out).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "alpha");
        assert!(installed[0].version.is_empty());
    }

    #[test]
    fn test_materialize_writes_and_prunes_only_owned_dirs() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        std::fs::write(
            src.join("alpha.md"),
            "---\nname: alpha\ndescription: a\n---\nalpha body\n",
        )
        .unwrap();
        std::fs::write(
            src.join("beta.md"),
            "---\nname: beta\ndescription: b\n---\nbeta body\n",
        )
        .unwrap();
        let source = src.to_str().unwrap();

        let collected =
            collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap();
        assert_eq!(collected.len(), 2);
        let dir_of = |name: &str| -> String {
            collected
                .iter()
                .find(|s| s.name == name)
                .unwrap()
                .dir_name
                .clone()
        };
        let alpha_dir = dir_of("alpha");
        let beta_dir = dir_of("beta");

        let out_dir = tempdir().unwrap();
        let out = out_dir.path();
        // A vendored checkout and a hand-written note: neither has an `@`, so
        // neither is ours to delete.
        std::fs::create_dir(out.join("vendored-upstream")).unwrap();
        std::fs::write(out.join("README.md"), "mine").unwrap();

        let written = materialize_skills(out, &collected).unwrap();
        assert_eq!(written.written.len(), 2);
        assert!(written.removed.is_empty());
        assert_eq!(
            std::fs::read_to_string(out.join(&alpha_dir).join("SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: a\n---\nalpha body\n"
        );

        // Re-running with the same desired set is a no-op on disk.
        let again = materialize_skills(out, &collected).unwrap();
        assert!(again.written.is_empty());
        assert!(again.removed.is_empty());

        // Narrowing the desired set removes only the dropped skill.
        let only_alpha: Vec<_> = collected
            .iter()
            .filter(|s| s.name == "alpha")
            .cloned()
            .collect();
        let narrowed = materialize_skills(out, &only_alpha).unwrap();
        assert_eq!(narrowed.removed, vec![beta_dir.clone()]);
        assert!(out.join(&alpha_dir).is_dir());
        assert!(!out.join(&beta_dir).exists());
        assert!(out.join("vendored-upstream").is_dir());
        assert!(out.join("README.md").is_file());
    }

    #[test]
    fn test_install_skills_with_fallbacks() {
        use tempfile::tempdir;
        let git_dir = tempdir().unwrap();
        let repo_path = git_dir.path();

        // 1. Initialize git repo
        std::process::Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // Set git config for local commit
        std::process::Command::new("git")
            .arg("config")
            .arg("user.name")
            .arg("Test User")
            .current_dir(repo_path)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("config")
            .arg("user.email")
            .arg("test@example.com")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // 2. Create skill files with missing name and description respectively
        let refactor_file = repo_path.join("refactor.md");
        std::fs::write(
            &refactor_file,
            "---\ndescription: Iterative refactoring loop\n---\nrefactor body\n",
        )
        .unwrap();

        let sdr_dir = repo_path.join("software-design-review");
        std::fs::create_dir(&sdr_dir).unwrap();
        let sdr_file = sdr_dir.join("SKILL.md");
        std::fs::write(
            &sdr_file,
            "---\nname: software-design-review\n---\nsdr body\n",
        )
        .unwrap();

        // 3. Commit files
        std::process::Command::new("git")
            .arg("add")
            .arg("refactor.md")
            .arg("software-design-review/SKILL.md")
            .current_dir(repo_path)
            .status()
            .unwrap();
        std::process::Command::new("git")
            .arg("commit")
            .arg("-m")
            .arg("add skills")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // Get HEAD commit hash
        let output = std::process::Command::new("git")
            .arg("rev-parse")
            .arg("HEAD")
            .current_dir(repo_path)
            .output()
            .unwrap();
        let head_commit = String::from_utf8(output.stdout).unwrap().trim().to_string();

        // 5. Clone and install
        let clone_temp_dir = tempdir().unwrap();
        let clone_path = clone_temp_dir.path();
        std::process::Command::new("git")
            .arg("clone")
            .arg(repo_path.to_str().unwrap())
            .arg(clone_path.to_str().unwrap())
            .status()
            .unwrap();

        let collected = collect_skills_from_dir(
            clone_path,
            repo_path.to_str().unwrap(),
            &head_commit,
            SelectionMode::All,
            false,
        )
        .unwrap();

        // 6. Verify skills collected correctly with name fallbacks
        assert_eq!(collected.len(), 2);

        let refactor = collected.iter().find(|s| s.name == "refactor").unwrap();
        assert_eq!(refactor.description, "Iterative refactoring loop");

        let sdr = collected
            .iter()
            .find(|s| s.name == "software-design-review")
            .unwrap();
        assert_eq!(sdr.description, "");
    }

    #[test]
    fn test_markdown_link_targets() {
        let content = "See [AGENT-BRIEF.md](AGENT-BRIEF.md) and \
             [a titled link](path/to.md \"a title\") and [anchor](#section) and \
             [site](https://example.com) plain text with no link.";
        assert_eq!(
            markdown_link_targets(content),
            vec![
                "AGENT-BRIEF.md",
                "path/to.md",
                "#section",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn test_backtick_path_targets() {
        let content = "See `references/schemas.md` for the schema. Also `grading.json` \
             and `evidence` are just field names, and `path with space.md` doesn't count.\n\
             ```\n\
             this fenced `inside.md` mention must not match\n\
             ```\n\
             But `after-fence.md` on a normal line does.";
        assert_eq!(
            backtick_path_targets(content),
            vec!["references/schemas.md", "after-fence.md"]
        );
    }

    #[test]
    fn test_is_local_reference() {
        assert!(is_local_reference("reference.md"));
        assert!(is_local_reference("../sibling/reference.md"));
        assert!(!is_local_reference("#section"));
        assert!(!is_local_reference("https://example.com/doc.md"));
        assert!(!is_local_reference("mailto:you@example.com"));
    }

    #[test]
    fn test_inline_references_pulls_in_sibling_docs() {
        use tempfile::tempdir;
        let root = tempdir().unwrap();
        let skill_dir = root.path().join("triage");
        std::fs::create_dir(&skill_dir).unwrap();

        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: triage\ndescription: toc-style skill\n---\n\
             # Triage\n\n## Reference docs\n\n\
             - [AGENT-BRIEF.md](AGENT-BRIEF.md) -- how to write agent briefs\n\
             - [OUT-OF-SCOPE.md](OUT-OF-SCOPE.md) -- rejected work log\n\n\
             Later: post an agent brief ([AGENT-BRIEF.md](AGENT-BRIEF.md)).\n",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("AGENT-BRIEF.md"),
            "# Agent Brief\n\nWrite briefs like this.\n",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("OUT-OF-SCOPE.md"),
            "# Out of Scope\n\nRejected requests live here.\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        let inlined = inline_references(&content, &skill_dir.join("SKILL.md"), root.path());

        assert!(inlined.contains("Write briefs like this."));
        assert!(inlined.contains("Rejected requests live here."));
        // Referenced twice in the source; inlined once (`visited` dedupes).
        assert_eq!(inlined.matches("Write briefs like this.").count(), 1);
    }

    #[test]
    fn test_inline_references_pulls_in_backtick_referenced_docs() {
        // Mirrors Anthropic's own skill-creator, which mentions references as
        // prose ("see `references/schemas.md`") rather than markdown links.
        use tempfile::tempdir;
        let root = tempdir().unwrap();
        let skill_dir = root.path().join("skill-creator");
        std::fs::create_dir(&skill_dir).unwrap();
        std::fs::create_dir(skill_dir.join("references")).unwrap();

        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: skill-creator\ndescription: toc-style skill\n---\n\
             See `references/schemas.md` for the full schema. This step \
             produces `benchmark.md`, which is not a file in this repo.\n",
        )
        .unwrap();
        std::fs::write(
            skill_dir.join("references/schemas.md"),
            "# Schemas\n\nThe JSON structures live here.\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        let inlined = inline_references(&content, &skill_dir.join("SKILL.md"), root.path());

        assert!(inlined.contains("The JSON structures live here."));
        // `benchmark.md` is path-shaped but never exists on disk -- silent no-op.
        assert!(!inlined.contains("inlined: skill-creator/benchmark.md"));
    }

    #[test]
    fn test_inline_references_skips_path_traversal() {
        use tempfile::tempdir;
        let outer = tempdir().unwrap();
        let root = outer.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(outer.path().join("secret.md"), "outside content").unwrap();

        let skill_file = root.join("SKILL.md");
        std::fs::write(
            &skill_file,
            "---\nname: x\n---\nSee [secret](../secret.md).\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&skill_file).unwrap();
        let inlined = inline_references(&content, &skill_file, &root);
        assert!(!inlined.contains("outside content"));
    }

    #[test]
    fn test_inline_references_skips_other_skill_entry_points() {
        use tempfile::tempdir;
        let root = tempdir().unwrap();
        let a_dir = root.path().join("skill-a");
        let b_dir = root.path().join("skill-b");
        std::fs::create_dir(&a_dir).unwrap();
        std::fs::create_dir(&b_dir).unwrap();

        std::fs::write(
            b_dir.join("SKILL.md"),
            "---\nname: skill-b\n---\nskill b's own body\n",
        )
        .unwrap();
        let a_file = a_dir.join("SKILL.md");
        std::fs::write(
            &a_file,
            "---\nname: skill-a\n---\nSee also [skill-b](../skill-b/SKILL.md).\n",
        )
        .unwrap();

        let content = std::fs::read_to_string(&a_file).unwrap();
        let inlined = inline_references(&content, &a_file, root.path());
        assert!(!inlined.contains("skill b's own body"));
    }

    #[test]
    fn test_install_from_dir_inlines_sibling_references() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: triage\ndescription: toc-style skill\n---\n\
             See [AGENT-BRIEF.md](AGENT-BRIEF.md) for the brief format.\n",
        )
        .unwrap();
        std::fs::write(
            src.join("AGENT-BRIEF.md"),
            "# Agent Brief\n\nWrite briefs like this.\n",
        )
        .unwrap();

        let source = src.to_str().unwrap();

        let collected =
            collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap();
        assert_eq!(collected.len(), 1);
        assert!(collected[0].body.contains("Write briefs like this."));
    }

    #[test]
    fn test_install_rejects_duplicate_skill_names() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        // Mirrors a real pattern: the same skill name declared under two
        // tool-specific directories, with genuinely different descriptions --
        // picking one silently would drop the other's content.
        std::fs::create_dir(src.join("skills")).unwrap();
        std::fs::create_dir(src.join(".opencode")).unwrap();
        std::fs::write(
            src.join("skills/one.md"),
            "---\nname: dup\ndescription: canonical copy\n---\nbody one\n",
        )
        .unwrap();
        std::fs::write(
            src.join(".opencode/two.md"),
            "---\nname: dup\ndescription: mirrored copy\n---\nbody two\n",
        )
        .unwrap();

        let source = src.to_str().unwrap();

        let err =
            collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("dup"), "message was: {message}");
        assert!(message.contains("one.md"), "message was: {message}");
        assert!(message.contains("two.md"), "message was: {message}");
    }

    #[test]
    fn test_install_select_ignores_unrelated_duplicate_names() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        // Same collision as above, plus one unambiguous skill elsewhere in the
        // same source. A narrow `--select` of the unambiguous one must still
        // succeed -- the collision only matters for the name it actually blocks.
        std::fs::create_dir(src.join("skills")).unwrap();
        std::fs::create_dir(src.join(".opencode")).unwrap();
        std::fs::write(
            src.join("skills/one.md"),
            "---\nname: dup\ndescription: canonical copy\n---\nbody one\n",
        )
        .unwrap();
        std::fs::write(
            src.join(".opencode/two.md"),
            "---\nname: dup\ndescription: mirrored copy\n---\nbody two\n",
        )
        .unwrap();
        std::fs::write(
            src.join("fine.md"),
            "---\nname: fine\ndescription: no conflict\n---\nfine body\n",
        )
        .unwrap();

        let source = src.to_str().unwrap();

        let outcome = collect_skills_from_dir(
            src,
            source,
            "v1",
            SelectionMode::Select(vec!["fine".to_string()]),
            false,
        )
        .unwrap();
        assert_eq!(outcome.len(), 1);
        assert_eq!(outcome[0].name, "fine");
    }
}
