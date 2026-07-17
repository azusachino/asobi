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

pub fn install_skills_from_dir<S: crate::api::SkillStore>(
    store: &S,
    dir_path: &Path,
    source: &str,
    version: &str,
    mode: SelectionMode,
    is_tty: bool,
    prune: bool,
) -> Result<()> {
    let mut parsed_skills = Vec::new();
    let mut skill_contents = HashMap::new();
    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let mut content = std::fs::read_to_string(entry.path())?.replace("\r\n", "\n");
        if let Some((parsed_name, parsed_desc)) = parse_frontmatter(&content) {
            let name = parsed_name.unwrap_or_else(|| resolve_skill_name_fallback(entry.path()));
            parsed_skills.push((name.clone(), parsed_desc.unwrap_or_default()));
            skill_contents.insert(name, std::mem::take(&mut content));
        }
    }
    if parsed_skills.is_empty() {
        bail!("No valid skills found in {}", source);
    }
    let selected_names = resolve_selection(&parsed_skills, mode, is_tty)?;
    let slug = derive_source_slug(source);
    if prune {
        let fresh: std::collections::HashSet<String> = selected_names
            .iter()
            .map(|n| crate::normalize::normalize_key(&format!("skill:{}:{}", slug, n)))
            .collect();
        let orphans = store
            .list_skills()?
            .into_iter()
            .filter(|s| derive_source_slug(&s.source) == slug && !fresh.contains(&s.entity_name))
            .map(|s| s.entity_name)
            .collect::<Vec<_>>();
        if !orphans.is_empty() {
            store.remove_skills(orphans)?;
        }
    }
    for name in selected_names {
        let body = skill_contents
            .remove(&name)
            .ok_or_else(|| anyhow!("Content missing for skill {}", name))?;
        let description = parsed_skills
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, d)| d.clone())
            .unwrap_or_default();
        store.upsert_skill(crate::api::SkillRecord {
            entity_name: crate::normalize::normalize_key(&format!("skill:{}:{}", slug, name)),
            body,
            source: source.to_string(),
            version: version.to_string(),
            description,
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::SkillStore;
    use crate::storage::Storage;

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

        // 4. Setup temp database
        let db_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                crate::paths::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let storage = Storage::open_default().unwrap();

        // 5. Clone and install
        let clone_temp_dir = tempdir().unwrap();
        let clone_path = clone_temp_dir.path();
        std::process::Command::new("git")
            .arg("clone")
            .arg(repo_path.to_str().unwrap())
            .arg(clone_path.to_str().unwrap())
            .status()
            .unwrap();

        install_skills_from_dir(
            &storage,
            clone_path,
            repo_path.to_str().unwrap(),
            &head_commit,
            SelectionMode::All,
            false,
            true,
        )
        .unwrap();

        // 6. Verify skill installed
        let skills = storage.list_skills().unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(
            skills[0].entity_name,
            crate::normalize::normalize_key(&format!(
                "skill:{}:repo-skill",
                derive_source_slug(repo_path.to_str().unwrap())
            ))
        );
        assert_eq!(skills[0].version, head_commit);
    }

    #[test]
    fn test_sync_prunes_orphaned_skills() {
        use tempfile::tempdir;
        let src_dir = tempdir().unwrap();
        let src = src_dir.path();

        // Initial source with two skills.
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

        let db_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                crate::paths::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let storage = Storage::open_default().unwrap();

        let source = src.to_str().unwrap();
        let slug = derive_source_slug(source);

        install_skills_from_dir(&storage, src, source, "v1", SelectionMode::All, false, true)
            .unwrap();
        assert_eq!(storage.list_skills().unwrap().len(), 2);

        // Upstream removes `beta`; a sync (install --all) must prune it.
        std::fs::remove_file(src.join("beta.md")).unwrap();

        install_skills_from_dir(&storage, src, source, "v2", SelectionMode::All, false, true)
            .unwrap();

        let skills = storage.list_skills().unwrap();
        assert_eq!(skills.len(), 1);
        let alpha = crate::normalize::normalize_key(&format!("skill:{}:alpha", slug));
        assert_eq!(skills[0].entity_name, alpha);
        assert_eq!(skills[0].version, "v2");
    }

    #[test]
    fn test_select_does_not_prune() {
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

        let db_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                crate::paths::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let storage = Storage::open_default().unwrap();
        let source = src.to_str().unwrap();

        // Install only alpha, then only beta — both must survive (additive).
        for name in ["alpha", "beta"] {
            install_skills_from_dir(
                &storage,
                src,
                source,
                "v1",
                SelectionMode::Select(vec![name.to_string()]),
                false,
                false,
            )
            .unwrap();
        }

        assert_eq!(storage.list_skills().unwrap().len(), 2);
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

        // 4. Setup temp database
        let db_dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                crate::paths::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let storage = Storage::open_default().unwrap();

        // 5. Clone and install
        let clone_temp_dir = tempdir().unwrap();
        let clone_path = clone_temp_dir.path();
        std::process::Command::new("git")
            .arg("clone")
            .arg(repo_path.to_str().unwrap())
            .arg(clone_path.to_str().unwrap())
            .status()
            .unwrap();

        install_skills_from_dir(
            &storage,
            clone_path,
            repo_path.to_str().unwrap(),
            &head_commit,
            SelectionMode::All,
            false,
            true,
        )
        .unwrap();

        // 6. Verify skills installed correctly with fallbacks
        let skills = storage.list_skills().unwrap();
        assert_eq!(skills.len(), 2);

        let slug = derive_source_slug(repo_path.to_str().unwrap());

        let refactor_entity = crate::normalize::normalize_key(&format!("skill:{}:refactor", slug));
        let sdr_entity =
            crate::normalize::normalize_key(&format!("skill:{}:software-design-review", slug));

        let refactor_row = skills
            .iter()
            .find(|s| s.entity_name == refactor_entity)
            .unwrap();
        assert_eq!(refactor_row.description, "Iterative refactoring loop");

        let sdr_row = skills.iter().find(|s| s.entity_name == sdr_entity).unwrap();
        assert_eq!(sdr_row.description, "");
    }
}
