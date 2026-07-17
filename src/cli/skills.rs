use super::commands::SkillsCommands;
use super::runtime::*;
use crate::api::v1::SkillStore;
use crate::paths::AsobiPaths;
use anyhow::Result;
use std::io::IsTerminal;
use tracing::{info, warn};

pub(crate) async fn run(
    backend: &crate::storage::Storage,
    paths: &AsobiPaths,
    subcommand: Option<SkillsCommands>,
) -> Result<()> {
    match subcommand {
        None => {
            let skills = backend.list_skills().await?;
            if skills.is_empty() {
                println!("No skills installed.");
            } else {
                let mut grouped: std::collections::BTreeMap<
                    String,
                    Vec<crate::api::v1::SkillRecord>,
                > = std::collections::BTreeMap::new();
                for s in skills {
                    grouped.entry(s.source.clone()).or_default().push(s);
                }
                println!("Installed Skills:");
                for (source, list) in grouped {
                    println!("Source: {}", source);
                    for s in list {
                        println!("  {} · {} · {}", s.entity_name, s.description, s.version);
                    }
                }
            }
        }
        Some(SkillsCommands::Install {
            source,
            all,
            select,
        }) => {
            let mut git_url = source.clone();
            let is_git = if source.contains("://") || source.contains("git@") {
                true
            } else if source.contains("github.com/") || source.contains("gitlab.com/") {
                git_url = format!("https://{}", source);
                true
            } else {
                !std::path::Path::new(&source).is_dir() && source.ends_with(".git")
            };

            let (target_path, version) = if is_git {
                let (cache_path, ver) = get_or_update_cached_repo(&git_url, &paths.caches_dir())?;
                (cache_path, ver)
            } else {
                let local_path = std::path::Path::new(&source);
                if !local_path.exists() {
                    anyhow::bail!("Local path {} does not exist", source);
                }
                (local_path.to_path_buf(), "local".to_string())
            };

            let mode = if all {
                crate::skills::SelectionMode::All
            } else if let Some(sel) = select {
                crate::skills::SelectionMode::Select(sel)
            } else {
                crate::skills::SelectionMode::Interactive
            };

            let is_tty = std::io::stdin().is_terminal();

            #[cfg(feature = "documents")]
            let embedder = init_embedder(paths)?;

            // `--all` is a full sync of the source: prune skills that
            // vanished upstream. `--select` / interactive stay additive.
            let prune = matches!(mode, crate::skills::SelectionMode::All);

            #[cfg(feature = "documents")]
            crate::skills::install_skills_from_store(
                backend,
                backend,
                embedder.as_ref(),
                &target_path,
                &git_url,
                &version,
                mode,
                is_tty,
                prune,
            )
            .await?;
            #[cfg(not(feature = "documents"))]
            crate::skills::install_skills_from_dir(
                backend,
                &target_path,
                &git_url,
                &version,
                mode,
                is_tty,
                prune,
            )
            .await?;

            info!("Skills installed successfully.");
        }
        Some(SkillsCommands::Update { source }) => {
            #[cfg(feature = "documents")]
            let embedder = init_embedder(paths)?;

            let skills = backend.list_skills().await?;
            let mut unique_sources = std::collections::HashSet::new();
            for s in skills {
                if let Some(ref filter_src) = source {
                    let slug = crate::skills::derive_source_slug(&s.source);
                    if &s.source == filter_src || &slug == filter_src {
                        unique_sources.insert(s.source.clone());
                    }
                } else {
                    unique_sources.insert(s.source.clone());
                }
            }

            if unique_sources.is_empty() {
                if let Some(src_val) = source {
                    anyhow::bail!(
                        "No installed skills found matching source/slug {:?}",
                        src_val
                    );
                } else {
                    info!("No skills currently installed.");
                    return Ok(());
                }
            }

            for src in unique_sources {
                info!("Updating skills from {}...", src);
                let mut git_url = src.clone();
                let is_git = if src.contains("://") || src.contains("git@") {
                    true
                } else if src.contains("github.com/") || src.contains("gitlab.com/") {
                    git_url = format!("https://{}", src);
                    true
                } else {
                    !std::path::Path::new(&src).is_dir() && src.ends_with(".git")
                };

                let (target_path, version) = if is_git {
                    let (cache_path, ver) =
                        get_or_update_cached_repo(&git_url, &paths.caches_dir())?;
                    (cache_path, ver)
                } else {
                    let local_path = std::path::Path::new(&src);
                    if !local_path.exists() {
                        warn!("Local path {} does not exist, skipping update", src);
                        continue;
                    }
                    (local_path.to_path_buf(), "local".to_string())
                };

                #[cfg(feature = "documents")]
                crate::skills::install_skills_from_store(
                    backend,
                    backend,
                    embedder.as_ref(),
                    &target_path,
                    &git_url,
                    &version,
                    crate::skills::SelectionMode::All,
                    false,
                    true,
                )
                .await?;
                #[cfg(not(feature = "documents"))]
                crate::skills::install_skills_from_dir(
                    backend,
                    &target_path,
                    &git_url,
                    &version,
                    crate::skills::SelectionMode::All,
                    false,
                    true,
                )
                .await?;
                info!("Successfully updated skills from {}.", src);
            }
        }
        Some(SkillsCommands::Remove { target }) => {
            let skills = backend.list_skills().await?;
            let mut entities_to_delete = Vec::new();
            for s in skills {
                let slug = crate::skills::derive_source_slug(&s.source);
                if s.entity_name == target || s.source == target || slug == target {
                    entities_to_delete.push(s.entity_name.clone());
                }
            }

            if !entities_to_delete.is_empty() {
                info!("Deleting {} skill entities...", entities_to_delete.len());
                backend.remove_skills(entities_to_delete).await?;
                info!("Skills removed successfully.");
            } else if target.starts_with("skill:") {
                info!("Deleting skill entity {}...", target);
                backend.remove_skills(vec![target.clone()]).await?;
                info!("Skills removed successfully.");
            } else {
                anyhow::bail!("No installed skills found matching target {:?}", target);
            }
        }
        Some(SkillsCommands::Show { name }) => {
            let mut entity_name = name.clone();
            if !entity_name.starts_with("skill:") {
                let skills = backend.list_skills().await?;
                let matches: Vec<_> = skills
                    .iter()
                    .filter(|s| {
                        s.entity_name == name || s.entity_name.ends_with(&format!(":{}", name))
                    })
                    .collect();
                if matches.len() == 1 {
                    entity_name = matches[0].entity_name.clone();
                } else if matches.len() > 1 {
                    anyhow::bail!(
                        "Ambiguous skill name '{}'. Matches: {}",
                        name,
                        matches
                            .iter()
                            .map(|s| &s.entity_name)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                } else {
                    entity_name = format!("skill:{}", name);
                }
            }

            match backend.skill_body(&entity_name).await? {
                Some(body) => {
                    print!("{}", body);
                }
                None => {
                    anyhow::bail!("Skill '{}' not found", name);
                }
            }
        }
    }

    Ok(())
}
