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

pub fn parse_frontmatter(content: &str) -> Option<(Option<String>, Option<String>)> {
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

    if found_end {
        return Some((name, description));
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

pub async fn install_skills_from_dir<
    #[cfg(feature = "documents")] E: crate::embed::EmbeddingProvider,
>(
    conn: &libsql::Connection,
    dir_path: &Path,
    source: &str,
    version: &str,
    mode: SelectionMode,
    is_tty: bool,
    prune: bool,
    #[cfg(feature = "documents")] vector_ctx: Option<(&crate::vector::VectorStore, &E)>,
) -> Result<()> {
    let mut parsed_skills = Vec::new();
    let mut skill_contents = HashMap::new();

    for entry in WalkDir::new(dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && e.path().extension().is_some_and(|ext| ext == "md"))
    {
        let mut content = std::fs::read_to_string(entry.path())?;
        content = content.replace("\r\n", "\n");
        if let Some((parsed_name, parsed_desc)) = parse_frontmatter(&content) {
            let name = match parsed_name {
                Some(n) => n,
                None => {
                    let file_stem = entry
                        .path()
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    if file_stem.eq_ignore_ascii_case("SKILL")
                        || file_stem.eq_ignore_ascii_case("index")
                    {
                        entry
                            .path()
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or(file_stem)
                            .to_string()
                    } else {
                        file_stem.to_string()
                    }
                }
            };
            let desc = parsed_desc.unwrap_or_default();
            parsed_skills.push((name.clone(), desc));
            skill_contents.insert(name, content);
        } else {
            // Check if it looks like it has frontmatter but is malformed
            if content.starts_with("---\n") {
                warn!("Malformed frontmatter in skill file {:?}", entry.path());
            }
        }
    }

    if parsed_skills.is_empty() {
        bail!("No valid skills found in {}", source);
    }

    let selected_names = resolve_selection(&parsed_skills, mode, is_tty)?;
    let slug = derive_source_slug(source);

    // Sync mode (update / install --all): drop skills previously installed from
    // this source that are no longer present upstream (deleted or renamed),
    // so the graph mirrors the source. `--select` stays purely additive.
    if prune {
        let fresh: std::collections::HashSet<String> = selected_names
            .iter()
            .map(|n| crate::normalize::normalize_key(&format!("skill:{}:{}", slug, n)))
            .collect();
        let orphans: Vec<String> = crate::db::list_skills(conn)
            .await?
            .into_iter()
            .filter(|s| derive_source_slug(&s.source) == slug && !fresh.contains(&s.entity_name))
            .map(|s| s.entity_name)
            .collect();
        if !orphans.is_empty() {
            warn!(
                "Pruning {} orphaned skill(s) from {}",
                orphans.len(),
                source
            );
            crate::db::delete_entities(conn, orphans).await?;
        }
    }

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

        let tx = conn.transaction().await?;

        // 1. Create the entity
        tx.execute(
            crate::constant::SQL_INSERT_ENTITY,
            libsql::params![entity_name.clone(), "skill".to_string()],
        )
        .await?;

        // 2. Set truths
        tx.execute(
            crate::constant::SQL_UPSERT_TRUTH,
            libsql::params![entity_name.clone(), "description".to_string(), description],
        )
        .await?;
        tx.execute(
            crate::constant::SQL_UPSERT_TRUTH,
            libsql::params![entity_name.clone(), "source".to_string(), source],
        )
        .await?;
        tx.execute(
            crate::constant::SQL_UPSERT_TRUTH,
            libsql::params![entity_name.clone(), "version".to_string(), version],
        )
        .await?;
        tx.execute(
            crate::constant::SQL_UPSERT_TRUTH,
            libsql::params![
                entity_name.clone(),
                "installed".to_string(),
                chrono::Utc::now().format("%Y-%m-%d").to_string()
            ],
        )
        .await?;

        // 3. Upsert into asobi_skills
        tx.execute(
            crate::constant::SQL_UPSERT_SKILL,
            libsql::params![entity_name.clone(), body.clone(), source, version],
        )
        .await?;

        tx.commit().await?;

        // 4. Chunk and embed into document store if available
        #[cfg(feature = "documents")]
        if let Some((store, embedder)) = vector_ctx {
            // Delete old chunks for this topic before re-indexing
            store.delete_by_topic(&entity_name).await?;
            crate::db::delete_topic(conn, &entity_name).await?;

            let texts = crate::chunk::chunk_text(&body, 512, 64);
            if !texts.is_empty() {
                let vectors = embedder.embed(&texts).await?;
                let chunks: Vec<crate::vector::Chunk> = texts
                    .into_iter()
                    .zip(vectors)
                    .enumerate()
                    .map(|(i, (text, vector))| crate::vector::Chunk {
                        id: uuid::Uuid::new_v4().to_string(),
                        topic_id: entity_name.clone(),
                        chunk_idx: i as u32,
                        text,
                        source: source.to_string(),
                        vector,
                    })
                    .collect();
                store.insert_chunks(chunks).await?;
            }
            crate::db::upsert_topic(conn, &entity_name, &name, source, &body).await?;
        }
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
            true,
            #[cfg(feature = "documents")]
            None::<(
                &crate::vector::VectorStore,
                &crate::embed::fastembed_provider::FastEmbedProvider,
            )>,
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

    #[cfg(feature = "documents")]
    #[tokio::test]
    async fn test_install_skills_document_embedding() {
        use tempfile::tempdir;
        let git_dir = tempdir().unwrap();
        let repo_path = git_dir.path();

        // 1. Initialize git repo
        std::process::Command::new("git")
            .arg("init")
            .current_dir(repo_path)
            .status()
            .unwrap();

        // Set git config
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

        // 2. Create a skill file with unique content
        let skill_file = repo_path.join("test-skill.md");
        std::fs::write(
            &skill_file,
            "---\nname: doc-skill\ndescription: searchable skill\n---\nHere is some unique knowledge about quantum cryptography.\n",
        )
        .unwrap();

        // 3. Commit
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

        // Initialize VectorStore and FakeEmbedder
        let store = crate::vector::VectorStore::new_with_dim(conn.clone(), 384);

        struct FakeEmbedder(usize);
        impl crate::embed::EmbeddingProvider for FakeEmbedder {
            async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
                Ok(texts.iter().map(|_| vec![0.1f32; self.0]).collect())
            }
            fn dim(&self) -> usize {
                self.0
            }
        }
        let embedder = FakeEmbedder(384);

        // 5. Clone and install passing vector context
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
            true,
            Some((&store, &embedder)),
        )
        .await
        .unwrap();

        // 6. Verify skill is queryable via recall
        let results = crate::recall::recall("cryptography", &conn, &store, &embedder, 5)
            .await
            .unwrap();
        assert!(!results.is_empty(), "expected skill to be queryable");

        // Topic ID should be the normalized skill entity name
        let slug = derive_source_slug(repo_path.to_str().unwrap());
        let expected_topic_id =
            crate::normalize::normalize_key(&format!("skill:{}:doc-skill", slug));
        assert_eq!(results[0].topic_id, expected_topic_id);
        assert_eq!(results[0].title, "doc-skill");
        assert!(results[0].snippet.contains("quantum cryptography"));
    }

    #[tokio::test]
    async fn test_sync_prunes_orphaned_skills() {
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
                crate::db::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = crate::db::init_db().await.unwrap();

        let source = src.to_str().unwrap();
        let slug = derive_source_slug(source);

        install_skills_from_dir(
            &conn,
            src,
            source,
            "v1",
            SelectionMode::All,
            false,
            true,
            #[cfg(feature = "documents")]
            None::<(
                &crate::vector::VectorStore,
                &crate::embed::fastembed_provider::FastEmbedProvider,
            )>,
        )
        .await
        .unwrap();
        assert_eq!(crate::db::list_skills(&conn).await.unwrap().len(), 2);

        // Upstream removes `beta`; a sync (install --all) must prune it.
        std::fs::remove_file(src.join("beta.md")).unwrap();

        install_skills_from_dir(
            &conn,
            src,
            source,
            "v2",
            SelectionMode::All,
            false,
            true,
            #[cfg(feature = "documents")]
            None::<(
                &crate::vector::VectorStore,
                &crate::embed::fastembed_provider::FastEmbedProvider,
            )>,
        )
        .await
        .unwrap();

        let skills = crate::db::list_skills(&conn).await.unwrap();
        assert_eq!(skills.len(), 1);
        let alpha = crate::normalize::normalize_key(&format!("skill:{}:alpha", slug));
        assert_eq!(skills[0].entity_name, alpha);
        assert_eq!(skills[0].version, "v2");
    }

    #[tokio::test]
    async fn test_select_does_not_prune() {
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
                crate::db::ENV_DATABASE_URL,
                db_dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = crate::db::init_db().await.unwrap();
        let source = src.to_str().unwrap();

        // Install only alpha, then only beta — both must survive (additive).
        for name in ["alpha", "beta"] {
            install_skills_from_dir(
                &conn,
                src,
                source,
                "v1",
                SelectionMode::Select(vec![name.to_string()]),
                false,
                false,
                #[cfg(feature = "documents")]
                None::<(
                    &crate::vector::VectorStore,
                    &crate::embed::fastembed_provider::FastEmbedProvider,
                )>,
            )
            .await
            .unwrap();
        }

        assert_eq!(crate::db::list_skills(&conn).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_install_skills_with_fallbacks() {
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
            true,
            #[cfg(feature = "documents")]
            None::<(
                &crate::vector::VectorStore,
                &crate::embed::fastembed_provider::FastEmbedProvider,
            )>,
        )
        .await
        .unwrap();

        // 6. Verify skills installed correctly with fallbacks
        let skills = crate::db::list_skills(&conn).await.unwrap();
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
