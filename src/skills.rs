use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionMode {
    All,
    Select(Vec<String>),
    Interactive,
}

pub fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let mut lines = content.lines();
    let first = lines.next()?.trim();
    if first != "---" {
        return None;
    }

    let mut name = None;
    let mut description = None;
    let mut found_end = false;

    for line in lines {
        let line = line.trim();
        if line == "---" {
            found_end = true;
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let key = k.trim();
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if key == "name" {
                name = Some(val);
            } else if key == "description" {
                description = Some(val);
            }
        }
    }

    if found_end && let (Some(n), Some(d)) = (name, description) {
        return Some((n, d));
    }
    None
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

pub async fn install_skills_from_dir(
    conn: &libsql::Connection,
    dir_path: &Path,
    source: &str,
    version: &str,
    mode: SelectionMode,
    is_tty: bool,
) -> Result<()> {
    let mut parsed_skills = Vec::new();
    let mut skill_contents = HashMap::new();

    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let content = std::fs::read_to_string(entry.path())?;
        if let Some((name, desc)) = parse_frontmatter(&content) {
            parsed_skills.push((name.clone(), desc));
            skill_contents.insert(name, content);
        } else {
            // Check if it looks like it has frontmatter but is malformed
            if content.starts_with("---\n") || content.starts_with("---\r\n") {
                warn!("Malformed frontmatter in skill file {:?}", entry.path());
            }
        }
    }

    if parsed_skills.is_empty() {
        bail!("No valid skills found in {}", source);
    }

    let selected_names = resolve_selection(&parsed_skills, mode, is_tty)?;
    let slug = derive_source_slug(source);

    for name in selected_names {
        let body = skill_contents
            .remove(&name)
            .ok_or_else(|| anyhow!("Content missing for skill {}", name))?;
        let description = parsed_skills
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, d)| d.as_str())
            .unwrap_or("");

        let entity_name = crate::normalize::normalize_key(&format!("skill:{}:{}", slug, name));

        // 1. Create the entity
        crate::db::mcp_create_entities(
            conn,
            vec![crate::mcp::EntityInput {
                name: entity_name.clone(),
                entity_type: "skill".to_string(),
                observations: vec![],
            }],
        )
        .await?;

        // 2. Set truths
        crate::db::truth_upsert(conn, &entity_name, "description", description).await?;
        crate::db::truth_upsert(conn, &entity_name, "source", source).await?;
        crate::db::truth_upsert(conn, &entity_name, "version", version).await?;
        crate::db::truth_upsert(
            conn,
            &entity_name,
            "installed",
            &chrono::Utc::now().format("%Y-%m-%d").to_string(),
        )
        .await?;

        // 3. Upsert into mcp_skills
        crate::db::skill_upsert(conn, &entity_name, &body, source, version).await?;
    }

    Ok(())
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
            Some(("my-skill".to_string(), "does something".to_string()))
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
        assert_eq!(parsed, None);
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

    #[tokio::test]
    async fn test_install_from_local_git_repo() {
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
                crate::db::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = crate::db::init_db().await.unwrap();

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
            &conn,
            clone_path,
            repo_path.to_str().unwrap(),
            &head_commit,
            SelectionMode::All,
            false,
        )
        .await
        .unwrap();

        // 6. Verify skill installed
        let skills = crate::db::list_skills(&conn).await.unwrap();
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
}
