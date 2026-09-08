use super::commands::SkillsCommands;
use super::runtime::*;
use crate::paths::AsobiPaths;
use anyhow::Result;
use std::io::IsTerminal;
use tracing::{info, warn};

/// A source resolved to something installable.
struct Checkout {
    /// Directory holding the source's skill files.
    path: std::path::PathBuf,
    /// Version to record: the git commit, or `local` for a path source.
    version: String,
    /// The canonical source string to record on each skill.
    url: String,
}

/// Resolve a source to a directory holding its skills. Git sources go through
/// the shared clone cache; local paths are used in place.
fn checkout_source(source: &str, caches_dir: &std::path::Path) -> Result<Checkout> {
    let (url, is_git) = classify_skill_source(source);
    let (path, version) = if is_git {
        get_or_update_cached_repo(&url, caches_dir)?
    } else {
        let local_path = std::path::Path::new(source);
        if !local_path.exists() {
            anyhow::bail!("Local path {} does not exist", source);
        }
        (local_path.to_path_buf(), "local".to_string())
    };
    Ok(Checkout { path, version, url })
}

/// The directory to actually walk for skills: the checkout root, or a
/// declared subdirectory of it. Checked eagerly so a typo'd `subdir` fails
/// with a specific message rather than the walk silently finding nothing and
/// later erroring with the generic "No valid skills found".
fn scoped_dir(
    checkout_path: &std::path::Path,
    subdir: Option<&std::path::Path>,
) -> Result<std::path::PathBuf> {
    let Some(subdir) = subdir else {
        return Ok(checkout_path.to_path_buf());
    };
    let scoped = checkout_path.join(subdir);
    if !scoped.is_dir() {
        anyhow::bail!(
            "subdir '{}' does not exist under {}",
            subdir.display(),
            checkout_path.display()
        );
    }
    Ok(scoped)
}

fn classify_skill_source(source: &str) -> (String, bool) {
    let mut git_url = source.to_string();
    let is_git = if source.contains("://") || source.contains("git@") {
        true
    } else if source.contains("github.com/") || source.contains("gitlab.com/") {
        git_url = format!("https://{source}");
        true
    } else {
        !std::path::Path::new(source).is_dir() && source.ends_with(".git")
    };
    (git_url, is_git)
}

/// Where installed skills live: the `[skills]` block's `path` when an
/// `asobi.toml` declares one, otherwise `.agents/skills` under the discovered
/// root.
///
/// The fallback matters. `asobi init` without `--local` writes no `asobi.toml`
/// at all, so requiring the config here would leave the default XDG install
/// with no way to reach its own skills.
fn skills_dir(paths: &AsobiPaths) -> Result<std::path::PathBuf> {
    if let Some(config_file) = paths.config_file.as_ref()
        && let Some(config) = crate::skills_config::SkillsConfig::load(config_file)?
    {
        return Ok(config.resolved_path(&paths.root));
    }
    Ok(paths.root.join(".agents/skills"))
}

/// Collect from one source and write the result to disk, replacing whatever
/// that source had installed before.
fn sync_sources(
    dir: &std::path::Path,
    collected: Vec<crate::skills::CollectedSkill>,
) -> Result<crate::skills::MaterializeOutcome> {
    crate::skills::materialize_skills(dir, &collected)
}

pub(crate) fn run(paths: &AsobiPaths, subcommand: Option<SkillsCommands>) -> Result<()> {
    let dir = skills_dir(paths)?;
    match subcommand {
        None => {
            let skills = crate::skills::read_installed_skills(&dir)?;
            if skills.is_empty() {
                println!("No skills installed in {}.", dir.display());
                return Ok(());
            }
            let mut grouped: std::collections::BTreeMap<String, Vec<_>> = Default::default();
            for s in skills {
                grouped
                    .entry(if s.source.is_empty() {
                        "(unknown source)".to_string()
                    } else {
                        s.source.clone()
                    })
                    .or_default()
                    .push(s);
            }
            println!("Installed Skills ({}):", dir.display());
            for (source, list) in grouped {
                println!("Source: {}", source);
                for s in list {
                    let version = if s.version.is_empty() {
                        "unrecorded".to_string()
                    } else {
                        s.version.clone()
                    };
                    println!("  {} · {} · {}", s.name, s.description, version);
                }
            }
        }
        Some(SkillsCommands::Install {
            source,
            all,
            select,
            subdir,
        }) => {
            let checkout = checkout_source(&source, &paths.caches_dir())?;
            let walk_dir = scoped_dir(&checkout.path, subdir.as_deref())?;
            let mode = if all {
                crate::skills::SelectionMode::All
            } else if let Some(sel) = select {
                crate::skills::SelectionMode::Select(sel)
            } else {
                crate::skills::SelectionMode::Interactive
            };
            let fresh = crate::skills::collect_skills_from_dir(
                &walk_dir,
                &checkout.url,
                &checkout.version,
                mode,
                std::io::stdin().is_terminal(),
            )?;

            // Installing is additive across sources: keep what other sources
            // put here, replace only this source's own skills. `--all` is a
            // full sync of *this* source, so anything it dropped upstream goes.
            let slug = crate::skills::derive_source_slug(&checkout.url);
            let mut desired: Vec<_> = crate::skills::read_installed_skills(&dir)?
                .into_iter()
                .filter(|s| crate::skills::derive_source_slug(&s.source) != slug)
                .filter_map(|s| reload(&dir, &s))
                .collect();
            desired.extend(fresh);
            let written = sync_sources(&dir, desired)?;
            info!(
                "Installed into {} ({} written, {} removed)",
                dir.display(),
                written.written.len(),
                written.removed.len()
            );
        }
        Some(SkillsCommands::Sync) => {
            let config_file = paths.config_file.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "no asobi.toml found; `skills sync` reads its `[skills]` block \
                     (create one with `asobi init --local`)"
                )
            })?;
            let config =
                crate::skills_config::SkillsConfig::load(config_file)?.ok_or_else(|| {
                    anyhow::anyhow!("{} declares no `[skills]` block", config_file.display())
                })?;
            if config.sources.is_empty() {
                anyhow::bail!(
                    "{} declares no `[[skills.source]]` entries",
                    config_file.display()
                );
            }

            // Validate the whole declaration before touching anything, so a
            // typo in the last source does not leave a half-applied sync.
            let selections = config
                .sources
                .iter()
                .map(|s| s.selection())
                .collect::<Result<Vec<_>>>()?;

            let mut desired = Vec::new();
            for (declared, mode) in config.sources.iter().zip(selections) {
                let checkout = checkout_source(&declared.url, &paths.caches_dir())?;
                let walk_dir = scoped_dir(&checkout.path, declared.subdir.as_deref())?;
                let collected = crate::skills::collect_skills_from_dir(
                    &walk_dir,
                    &checkout.url,
                    &checkout.version,
                    mode,
                    false,
                )?;
                info!("{}: {} selected", declared.url, collected.len());
                desired.extend(collected);
            }

            // The config is the whole truth: materialize prunes every skill
            // directory it did not just write, so a source dropped from the
            // config leaves the tree without any separate bookkeeping.
            let written = sync_sources(&dir, desired)?;
            info!(
                "Synced into {} ({} written, {} removed)",
                dir.display(),
                written.written.len(),
                written.removed.len()
            );
        }
        Some(SkillsCommands::Update { source }) => {
            let installed = crate::skills::read_installed_skills(&dir)?;
            let sources: std::collections::BTreeSet<String> = installed
                .iter()
                .filter(|s| !s.source.is_empty())
                .filter(|s| match source.as_ref() {
                    None => true,
                    Some(filter) => {
                        &s.source == filter
                            || &crate::skills::derive_source_slug(&s.source) == filter
                    }
                })
                .map(|s| s.source.clone())
                .collect();

            if sources.is_empty() {
                match source {
                    Some(val) => anyhow::bail!(
                        "No installed skills found matching source/slug {:?} in {}",
                        val,
                        dir.display()
                    ),
                    None => {
                        info!("No skills with a recorded source in {}.", dir.display());
                        return Ok(());
                    }
                }
            }

            // Refreshed sources are re-collected; everything else is carried
            // over untouched so a scoped update never prunes a sibling source.
            let mut desired = Vec::new();
            for s in &installed {
                if !sources.contains(&s.source)
                    && let Some(kept) = reload(&dir, s)
                {
                    desired.push(kept);
                }
            }
            for src in sources {
                info!("Updating skills from {}...", src);
                let (git_url, is_git) = classify_skill_source(&src);
                let (target_path, version) = if is_git {
                    get_or_update_cached_repo(&git_url, &paths.caches_dir())?
                } else {
                    let local_path = std::path::Path::new(&src);
                    if !local_path.exists() {
                        warn!("Local path {} does not exist, skipping update", src);
                        continue;
                    }
                    (local_path.to_path_buf(), "local".to_string())
                };
                desired.extend(crate::skills::collect_skills_from_dir(
                    &target_path,
                    &git_url,
                    &version,
                    crate::skills::SelectionMode::All,
                    false,
                )?);
            }
            let written = sync_sources(&dir, desired)?;
            info!(
                "Updated {} ({} written, {} removed)",
                dir.display(),
                written.written.len(),
                written.removed.len()
            );
        }
        Some(SkillsCommands::Remove { target }) => {
            let installed = crate::skills::read_installed_skills(&dir)?;
            let (dropped, kept): (Vec<_>, Vec<_>) = installed.into_iter().partition(|s| {
                s.name == target
                    || s.dir == target
                    || s.source == target
                    || crate::skills::derive_source_slug(&s.source) == target
            });
            if dropped.is_empty() {
                anyhow::bail!(
                    "No installed skills found matching target {:?} in {}",
                    target,
                    dir.display()
                );
            }
            let desired: Vec<_> = kept.iter().filter_map(|s| reload(&dir, s)).collect();
            let written = sync_sources(&dir, desired)?;
            info!(
                "Removed {} skill(s) from {}",
                written.removed.len(),
                dir.display()
            );
        }
        Some(SkillsCommands::Show { name }) => {
            let installed = crate::skills::read_installed_skills(&dir)?;
            let matches: Vec<_> = installed
                .iter()
                .filter(|s| s.name == name || s.dir == name)
                .collect();
            match matches.as_slice() {
                [one] => print!(
                    "{}",
                    std::fs::read_to_string(dir.join(&one.dir).join("SKILL.md"))?
                ),
                [] => anyhow::bail!("Skill '{}' not found in {}", name, dir.display()),
                many => anyhow::bail!(
                    "Ambiguous skill name '{}'. Matches: {}",
                    name,
                    many.iter()
                        .map(|s| s.dir.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    }

    Ok(())
}

/// Re-read an already-installed skill's body so it can be carried through a
/// rewrite of the tree untouched.
fn reload(
    dir: &std::path::Path,
    installed: &crate::skills::InstalledSkill,
) -> Option<crate::skills::CollectedSkill> {
    let body = std::fs::read_to_string(dir.join(&installed.dir).join("SKILL.md")).ok()?;
    Some(crate::skills::CollectedSkill {
        dir_name: installed.dir.clone(),
        name: installed.name.clone(),
        description: installed.description.clone(),
        source: installed.source.clone(),
        version: installed.version.clone(),
        body,
    })
}
