use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
    /// Canonical source URL or path this came from.
    pub source: String,
    /// Resolved git commit, or `local` for a path source.
    pub version: String,
    /// The full `SKILL.md` text, frontmatter included.
    pub body: String,
    /// The skill's own directory in the source checkout. Every skill has one:
    /// a skill *is* a directory containing `SKILL.md`. Its Markdown is copied
    /// alongside the body so on-demand references resolve after install.
    pub bundle_dir: std::path::PathBuf,
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
    let mut skill_contents: HashMap<String, (String, std::path::PathBuf)> = HashMap::new();
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
    // A skill is a directory containing `SKILL.md`, which is what the Agent
    // Skills specification defines and the only shape supported here. Loose
    // `<name>.md` files were once accepted too, which bought a name-fallback
    // rule, an optional bundle, and a class of skill whose relative references
    // could never resolve -- three kinds of complexity for a shape the spec
    // does not have.
    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| {
            e.path()
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("SKILL.md"))
        })
    {
        let Some(bundle_dir) = entry.path().parent().map(Path::to_path_buf) else {
            continue;
        };
        let content = std::fs::read_to_string(entry.path())?.replace("\r\n", "\n");
        if let Some((parsed_name, parsed_desc)) = parse_frontmatter(&content) {
            // The spec requires a skill's directory to be named for it, so the
            // directory is the right answer when frontmatter omits the name.
            let name = parsed_name.unwrap_or_else(|| {
                bundle_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("skill")
                    .to_string()
            });
            skill_paths
                .entry(name.clone())
                .or_default()
                .push(entry.path().to_path_buf());
            parsed_skills.push((name.clone(), parsed_desc.unwrap_or_default()));
            skill_contents.insert(name, (content, bundle_dir));
        }
    }
    if parsed_skills.is_empty() {
        bail!(
            "No skills found in {source}: a skill is a directory containing SKILL.md \
             (see https://agentskills.io/specification)"
        );
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
        let (body, bundle_dir) = skill_contents
            .remove(&name)
            .ok_or_else(|| anyhow!("Content missing for skill {}", name))?;
        collected.push(CollectedSkill {
            dir_name: format!(
                "{}@{}",
                crate::normalize::slugify(&slug),
                crate::normalize::slugify(&name)
            ),
            name,
            source: source.to_string(),
            version: version.to_string(),
            body,
            bundle_dir,
        });
    }
    Ok(collected)
}

/// A skill as recorded on disk, for `skills` and `skills show`.
///
/// Carries no `description`. It used to, and it was the one field the manifest
/// could get *wrong*: `dir` is derived, `source` and `version` come from git,
/// but `description` was re-serialized out of frontmatter by
/// [`crate::frontmatter`], a deliberately narrow subset with no support for
/// multi-line scalars — so a skill declaring `description: >` had the literal
/// `">"` recorded and then reported by `skills`. Widening the parser to fix a
/// field nothing depends on is the wrong trade; the description already sits
/// in the `SKILL.md` beside this record, which `skills show` prints in full.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSkill {
    /// On-disk directory name: `<source-slug>@<skill-name>`.
    pub dir: String,
    pub name: String,
    /// Canonical source URL or path.
    #[serde(default)]
    pub source: String,
    /// The git commit this was taken from, or `local`. Empty when the manifest
    /// was missing and this entry was recovered by scanning.
    #[serde(default)]
    pub version: String,
}

/// The two paths a skills tree spans: the directory agents read, and the
/// manifest recording where its contents came from.
///
/// Until 0.7 a skill's source and version lived on its graph entity, so with
/// the graph copy gone they need a home on disk — and the previous arrangement
/// recorded them so poorly that every installed skill in practice carried only
/// a `description` and no version at all. Recording them makes `skills` report
/// what commit each skill came from, and lets `update` and `remove` find a
/// source again after the fact. Pruning does *not* use the manifest: it works
/// off the `@` directory convention, in [`materialize_skills`].
///
/// The manifest is state, so it belongs under `data_dir` rather than in the
/// skills directory, which asobi treats as project content (see
/// `AsobiPaths::root`). The two are anchored differently — under the XDG
/// fallback `data_dir` is global while the skills directory follows the
/// working directory — so the file is keyed by the directory it describes.
/// Without that key every project sharing an XDG state tree would read, and
/// overwrite, one another's manifest.
pub struct SkillsTree {
    pub dir: PathBuf,
    pub manifest: PathBuf,
}

impl SkillsTree {
    pub fn new(data_dir: &Path, skills_dir: &Path) -> Self {
        Self {
            dir: skills_dir.to_path_buf(),
            manifest: data_dir.join(format!("skills-{:016x}.json", path_key(skills_dir))),
        }
    }
}

/// FNV-1a over the path's bytes. Written out rather than taken from
/// `DefaultHasher`, whose output is explicitly not stable across Rust
/// releases — a toolchain bump must not orphan every manifest on disk. The
/// path is used as given, without canonicalizing: it is already absolute (it
/// is resolved against `AsobiPaths::root`), and resolving symlinks would key
/// the same directory differently depending on whether it existed yet.
fn path_key(path: &Path) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Manifest {
    skills: Vec<InstalledSkill>,
}

/// Read the installed-skill manifest for `tree`.
///
/// Falls back to scanning `<slug>@<name>` directories when the manifest is
/// absent — a tree a human edited, or one written before the manifest
/// existed — so listing still works, at the cost of empty `source`/`version`.
/// Directories without `@` are ignored either way, the same convention
/// [`materialize_skills`] prunes by.
pub fn read_installed_skills(tree: &SkillsTree) -> Result<Vec<InstalledSkill>> {
    let dir = tree.dir.as_path();
    if let Ok(raw) = std::fs::read_to_string(&tree.manifest)
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
        let (name, _) = parse_frontmatter(&content).unwrap_or((None, None));
        found.push(InstalledSkill {
            name: name.unwrap_or_else(|| {
                dir_name
                    .split_once('@')
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| dir_name.clone())
            }),
            dir: dir_name,
            source: String::new(),
            version: String::new(),
        });
    }
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(found)
}

/// Copy a skill's bundled resources — `references/`, `scripts/`, `assets/`, and
/// anything else it ships — from the source checkout into its installed
/// directory. `SKILL.md` itself is skipped; the caller writes that from the
/// collected body.
///
/// The Agent Skills spec loads these on demand, only when the body points at
/// them, which is the whole of its progressive disclosure. Installing the body
/// without them leaves a skill whose instructions reference files that are not
/// there — the failure is silent, since nothing reads the body until an agent
/// does.
fn copy_bundle(skill: &CollectedSkill, to: &Path) -> Result<()> {
    let from = skill.bundle_dir.as_path();
    let mut skipped = Vec::new();
    for entry in WalkDir::new(from).into_iter().filter_map(|e| e.ok()) {
        let relative = entry.path().strip_prefix(from).unwrap_or(entry.path());
        if relative.as_os_str().is_empty() {
            continue;
        }
        // The entry point is written from the collected body, not copied, so a
        // sibling `index.md` alongside a `SKILL.md` still comes across.
        if relative.as_os_str().eq_ignore_ascii_case("SKILL.md") {
            continue;
        }
        if entry.file_type().is_dir() {
            continue;
        }
        if !entry.file_type().is_file() {
            // Symlinks are deliberately not followed: a skill source is
            // untrusted input, and a link out of the checkout would copy
            // whatever it names into the skills tree.
            continue;
        }
        if !is_markdown(entry.path()) {
            skipped.push(relative.display().to_string());
            continue;
        }
        let target = to.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Same mtime reasoning as the body: skip a byte-identical file.
        let unchanged = std::fs::read(entry.path())
            .ok()
            .zip(std::fs::read(&target).ok())
            .is_some_and(|(src, dst)| src == dst);
        if !unchanged {
            std::fs::copy(entry.path(), &target)?;
        }
    }

    if !skipped.is_empty() {
        skipped.sort();
        tracing::warn!(
            "skill '{}': did not install {} non-Markdown file(s) ({}). Asobi installs \
             instructions, not executables or data -- fetch them from the source if a \
             step genuinely needs them.",
            skill.name,
            skipped.len(),
            skipped.join(", ")
        );
    }
    Ok(())
}

/// Markdown is the only thing a skill install writes besides `SKILL.md`.
///
/// The rest of a skill's directory -- `scripts/`, `assets/`, tool-specific
/// config -- is fetched from a git URL and is exactly where the published
/// attack research finds payloads hidden, precisely because scanners read the
/// body and not the artifacts beside it. Writing an upstream script to disk on
/// the strength of a `select` line is a bigger promise than a skill installer
/// should make; a human who wants one can fetch it deliberately.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// Write `desired` out as `<dir>/<slug>@<name>/SKILL.md` plus its bundled
/// resources, then remove every other `<slug>@<name>` directory under `dir`.
///
/// Pruning is deliberately scoped to the `@` naming convention: directories
/// Asobi did not write — a hand-authored skill, a vendored upstream checkout —
/// have no `@` in their name and are never touched. Since 0.7 this is the only
/// place a skill is stored, so this pruning pass is also what retires a skill
/// the config stopped declaring or a source dropped upstream.
pub fn materialize_skills(
    tree: &SkillsTree,
    desired: &[CollectedSkill],
) -> Result<MaterializeOutcome> {
    let dir = tree.dir.as_path();
    let mut outcome = MaterializeOutcome::default();
    let wanted: std::collections::HashSet<&str> =
        desired.iter().map(|s| s.dir_name.as_str()).collect();

    std::fs::create_dir_all(dir)?;

    for skill in desired {
        let dir_name = &skill.dir_name;
        let skill_dir = dir.join(dir_name);
        let file = skill_dir.join("SKILL.md");
        std::fs::create_dir_all(&skill_dir)?;
        // Bundled resources are refreshed every run: an upstream change to a
        // `references/` file does not necessarily change `SKILL.md`, so gating
        // the copy on the body would let them drift.
        copy_bundle(skill, &skill_dir)?;
        // Leave an already-current body alone so a no-op sync does not churn
        // mtimes that file watchers key off.
        if std::fs::read_to_string(&file).is_ok_and(|existing| existing == skill.body) {
            continue;
        }
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
                source: s.source.clone(),
                version: s.version.clone(),
            })
            .collect(),
    };
    if let Some(parent) = tree.manifest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &tree.manifest,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;

    outcome.written.sort();
    outcome.removed.sort();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a spec-shaped skill: a directory named for it, holding SKILL.md.
    fn write_skill(root: &Path, name: &str, body: &str) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {name} description\n---\n{body}\n"),
        )
        .unwrap();
        dir
    }

    /// A skills tree whose manifest lives in a sibling state directory, the
    /// way `data_dir` and the skills directory actually sit apart on disk.
    /// Returns the skills directory too, since most assertions are about it.
    fn tree_under(root: &Path) -> (PathBuf, SkillsTree) {
        let skills = root.join("skills");
        std::fs::create_dir_all(&skills).unwrap();
        (
            skills.clone(),
            SkillsTree::new(&root.join("state"), &skills),
        )
    }

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
        let skill_file = repo_path.join("repo-skill").join("SKILL.md");
        std::fs::create_dir_all(skill_file.parent().unwrap()).unwrap();
        std::fs::write(
            &skill_file,
            "---\nname: repo-skill\ndescription: cloned skill\n---\nbody text\n",
        )
        .unwrap();

        // 3. Commit the file
        std::process::Command::new("git")
            .arg("add")
            .arg("repo-skill/SKILL.md")
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
        write_skill(src, "alpha", "alpha body");
        write_skill(src, "beta", "beta body");
        let source = src.to_str().unwrap();
        let out_dir = tempdir().unwrap();
        let (_out, tree) = tree_under(out_dir.path());

        let first = collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap();
        assert_eq!(first.len(), 2);
        materialize_skills(&tree, &first).unwrap();
        assert_eq!(read_installed_skills(&tree).unwrap().len(), 2);

        // Upstream removes `beta`; the next sync must drop it from disk.
        std::fs::remove_dir_all(src.join("beta")).unwrap();
        let second = collect_skills_from_dir(src, source, "v2", SelectionMode::All, false).unwrap();
        let outcome = materialize_skills(&tree, &second).unwrap();

        assert_eq!(outcome.removed.len(), 1);
        let installed = read_installed_skills(&tree).unwrap();
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
        write_skill(src, "alpha", "alpha body");
        let out_dir = tempdir().unwrap();
        let (out, tree) = tree_under(out_dir.path());
        let out = out.as_path();
        let collected = collect_skills_from_dir(
            src,
            "https://example.com/o/r.git",
            "abc123",
            SelectionMode::All,
            false,
        )
        .unwrap();
        materialize_skills(&tree, &collected).unwrap();

        assert!(tree.manifest.is_file());
        assert!(!out.join(".asobi-skills.json").exists());
        let installed = read_installed_skills(&tree).unwrap();
        assert_eq!(installed[0].source, "https://example.com/o/r.git");
        assert_eq!(installed[0].version, "abc123");
    }

    /// A folded or literal block scalar is the shape that made recording a
    /// description untenable: `crate::frontmatter` is a narrow subset that
    /// reads `description: >` as the literal `">"`. The manifest must not
    /// carry the field at all, so no such value can reach it.
    #[test]
    fn test_manifest_records_no_description() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        let folded = src.join("folded");
        std::fs::create_dir_all(&folded).unwrap();
        std::fs::write(
            folded.join("SKILL.md"),
            "---\nname: folded\ndescription: >\n  wrapped over\n  two lines\n---\nbody\n",
        )
        .unwrap();

        let out_dir = tempdir().unwrap();
        let (_out, tree) = tree_under(out_dir.path());
        let collected =
            collect_skills_from_dir(src, "local", "v1", SelectionMode::All, false).unwrap();
        materialize_skills(&tree, &collected).unwrap();

        let raw = std::fs::read_to_string(&tree.manifest).unwrap();
        assert!(!raw.contains("description"), "manifest carried: {raw}");
        assert!(
            !raw.contains('>'),
            "manifest carried a folded marker: {raw}"
        );
        assert_eq!(read_installed_skills(&tree).unwrap()[0].name, "folded");
    }

    /// Two skills directories sharing one `data_dir` — what the XDG fallback
    /// produces for any two projects on a machine, since `data_dir` is global
    /// there while the skills directory follows the working directory. Each
    /// must get its own manifest, or one project's sync silently rewrites the
    /// other's provenance.
    #[test]
    fn test_manifests_are_keyed_per_skills_dir() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        write_skill(src, "alpha", "alpha body");
        let collected = collect_skills_from_dir(
            src,
            "https://example.com/o/r.git",
            "v1",
            SelectionMode::All,
            false,
        )
        .unwrap();

        let root = tempdir().unwrap();
        let data = root.path().join("state");
        let one = SkillsTree::new(&data, &root.path().join("a/.agents/skills"));
        let two = SkillsTree::new(&data, &root.path().join("b/.agents/skills"));
        assert_ne!(one.manifest, two.manifest);

        materialize_skills(&one, &collected).unwrap();
        // Nothing is installed under `two`, so its own manifest is absent and
        // it must not inherit `one`'s.
        assert!(!two.manifest.exists());
        assert!(read_installed_skills(&two).unwrap().is_empty());
        assert_eq!(read_installed_skills(&one).unwrap().len(), 1);
    }

    /// The key must not move under a toolchain upgrade, or every manifest on
    /// disk is orphaned at once. Pinned so a swap away from FNV-1a is a
    /// deliberate, visible change rather than a silent one.
    #[test]
    fn test_path_key_is_stable() {
        assert_eq!(
            path_key(Path::new("/home/u/p/.agents/skills")),
            0x06e8_0bce_a145_c5e8
        );
    }

    /// Without a manifest — an older tree, or one a human pruned by hand —
    /// listing still works off the directory names, minus the provenance.
    #[test]
    fn test_read_installed_falls_back_to_scanning() {
        use tempfile::tempdir;
        let out_dir = tempdir().unwrap();
        let (out, tree) = tree_under(out_dir.path());
        let out = out.as_path();
        let dir = out.join("some-source@alpha");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: alpha\ndescription: a\n---\nbody\n",
        )
        .unwrap();

        let installed = read_installed_skills(&tree).unwrap();
        assert_eq!(installed.len(), 1);
        assert_eq!(installed[0].name, "alpha");
        assert!(installed[0].version.is_empty());
    }

    #[test]
    fn test_materialize_writes_and_prunes_only_owned_dirs() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        write_skill(src, "alpha", "alpha body");
        write_skill(src, "beta", "beta body");
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
        let (out, tree) = tree_under(out_dir.path());
        let out = out.as_path();
        // A vendored checkout and a hand-written note: neither has an `@`, so
        // neither is ours to delete.
        std::fs::create_dir(out.join("vendored-upstream")).unwrap();
        std::fs::write(out.join("README.md"), "mine").unwrap();

        let written = materialize_skills(&tree, &collected).unwrap();
        assert_eq!(written.written.len(), 2);
        assert!(written.removed.is_empty());
        assert_eq!(
            std::fs::read_to_string(out.join(&alpha_dir).join("SKILL.md")).unwrap(),
            "---\nname: alpha\ndescription: alpha description\n---\nalpha body\n"
        );

        // Re-running with the same desired set is a no-op on disk.
        let again = materialize_skills(&tree, &collected).unwrap();
        assert!(again.written.is_empty());
        assert!(again.removed.is_empty());

        // Narrowing the desired set removes only the dropped skill.
        let only_alpha: Vec<_> = collected
            .iter()
            .filter(|s| s.name == "alpha")
            .cloned()
            .collect();
        let narrowed = materialize_skills(&tree, &only_alpha).unwrap();
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
        let refactor_file = repo_path.join("refactor").join("SKILL.md");
        std::fs::create_dir_all(refactor_file.parent().unwrap()).unwrap();
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
            .arg("refactor/SKILL.md")
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

        // 6. A skill with no `name:` falls back to its directory name, and one
        // with no `description:` is still collected -- neither field being
        // declared is a reason to drop a skill on the floor.
        assert_eq!(collected.len(), 2);
        assert!(collected.iter().any(|s| s.name == "refactor"));
        assert!(collected.iter().any(|s| s.name == "software-design-review"));
    }

    /// A skill that owns a directory ships its bundled resources with it.
    /// Until 0.7 those were folded into the body instead, which defeated the
    /// spec's progressive disclosure -- and covered only markdown, so a
    /// bundled script silently vanished.
    #[test]
    fn test_bundled_resources_are_copied_not_inlined() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path().join("triage");
        std::fs::create_dir_all(src.join("references")).unwrap();
        std::fs::create_dir_all(src.join("scripts")).unwrap();

        // Written directly rather than via the helper: this body carries a
        // reference link, which is the whole point of the test.
        std::fs::write(
            src.join("SKILL.md"),
            "---\nname: triage\ndescription: toc-style skill\n---\n\
             See [the brief format](references/AGENT-BRIEF.md).\n",
        )
        .unwrap();
        std::fs::write(
            src.join("references/AGENT-BRIEF.md"),
            "# Agent Brief\n\nWrite briefs like this.\n",
        )
        .unwrap();
        std::fs::write(src.join("scripts/extract.py"), "print('hi')\n").unwrap();

        let source = src.to_str().unwrap();
        let collected =
            collect_skills_from_dir(&src, source, "v1", SelectionMode::All, false).unwrap();
        assert_eq!(collected.len(), 1);

        // The body stays as authored -- the reference is a pointer, not content.
        assert!(collected[0].body.contains("references/AGENT-BRIEF.md"));
        assert!(!collected[0].body.contains("Write briefs like this."));

        let out_dir = tempdir().unwrap();
        let (out, tree) = tree_under(out_dir.path());
        let out = out.as_path();
        materialize_skills(&tree, &collected).unwrap();
        let installed = out.join(&collected[0].dir_name);

        // ...the Markdown it points at is there to be read...
        assert_eq!(
            std::fs::read_to_string(installed.join("references/AGENT-BRIEF.md")).unwrap(),
            "# Agent Brief\n\nWrite briefs like this.\n"
        );
        // ...and the script is not. Auxiliary artifacts are where the published
        // attack research finds payloads, so an install writes instructions and
        // leaves fetched executables where they came from.
        assert!(!installed.join("scripts/extract.py").exists());
    }

    /// A loose `<name>.md` is not a skill. Accepting them bought a name
    /// fallback, an optional bundle, and a shape whose relative references
    /// could never resolve once installed.
    #[test]
    fn test_loose_markdown_is_not_a_skill() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();
        std::fs::write(
            src.join("alpha.md"),
            "---\nname: alpha\ndescription: a\n---\nalpha body\n",
        )
        .unwrap();

        let err =
            collect_skills_from_dir(src, src.to_str().unwrap(), "v1", SelectionMode::All, false)
                .unwrap_err();
        assert!(
            err.to_string().contains("directory containing SKILL.md"),
            "message was: {err}"
        );
    }

    #[test]
    fn test_install_rejects_duplicate_skill_names() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        // Mirrors a real pattern: the same skill name declared under two
        // tool-specific directories, with genuinely different descriptions --
        // picking one silently would drop the other's content.
        write_skill(&src.join("skills"), "dup", "body one");
        write_skill(&src.join(".opencode"), "dup", "body two");

        let source = src.to_str().unwrap();

        let err =
            collect_skills_from_dir(src, source, "v1", SelectionMode::All, false).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("dup"), "message was: {message}");
        assert!(message.contains("skills/dup"), "message was: {message}");
        assert!(message.contains(".opencode/dup"), "message was: {message}");
    }

    #[test]
    fn test_install_select_ignores_unrelated_duplicate_names() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        // Same collision as above, plus one unambiguous skill elsewhere in the
        // same source. A narrow `--select` of the unambiguous one must still
        // succeed -- the collision only matters for the name it actually blocks.
        write_skill(&src.join("skills"), "dup", "body one");
        write_skill(&src.join(".opencode"), "dup", "body two");
        write_skill(src, "fine", "fine body");

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
