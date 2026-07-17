use super::commands::{Cli, Commands};
use super::output::*;
use crate::api::{BackupStore, GraphStore};
use crate::application::AsobiRuntime;
use crate::paths::AsobiPaths;
use anyhow::Result;
use clap::CommandFactory;
use tracing::info;

pub(crate) fn run_cli(cli: Cli) -> Result<()> {
    if let Commands::Completions { shell } = cli.command {
        let mut command = Cli::command();
        let shell: clap_complete::Shell = shell.into();
        clap_complete::generate(shell, &mut command, "asobi", &mut std::io::stdout());
        return Ok(());
    }
    if let Commands::Schema { command } = cli.command {
        emit_schema(command.as_deref())?;
        return Ok(());
    }

    // `init` is special: it runs before any DB or config resolution, since
    // its job is to create the workspace those subsystems need.
    if let Commands::Init { local } = cli.command {
        let cwd = std::env::current_dir()?;
        let target = if local {
            crate::init::InitTarget::Local
        } else {
            crate::init::InitTarget::Xdg
        };
        let report = crate::init::init_workspace(target, &cwd)?;
        print_init_report(&report);
        return Ok(());
    }

    let paths = AsobiPaths::resolve();
    let runtime = AsobiRuntime::open_default()?;
    if let Commands::Restore { ref file, force } = cli.command {
        runtime
            .into_storage()
            .restore(std::path::PathBuf::from(file), force)?;
        return Ok(());
    }
    let backend = runtime.storage();

    let json = cli.json;
    match cli.command {
        Commands::Compact { older_than } => {
            let pruned = crate::compact::prune_old_sessions(
                &paths.topics_dir.to_string_lossy(),
                older_than,
            )?;
            info!("Pruned {} old session files.", pruned);
            let synced = crate::compact::sync_graph_to_markdown(backend)?;
            info!("Done. Synced {} entities to Markdown.", synced);
        }
        Commands::Tasks { subcommand } => crate::tasks::run(backend, subcommand, json)?,
        Commands::Skills { subcommand } => super::skills::run(backend, &paths, subcommand)?,
        command => super::graph::run(backend, command, json)?,
    }

    Ok(())
}

pub(crate) fn import_graph(store: &impl GraphStore, graph: crate::model::Graph) -> Result<()> {
    let mut entities = Vec::with_capacity(graph.entities.len());
    let mut truths = Vec::new();
    for entity in graph.entities {
        let name = entity.name;
        truths.extend(
            entity
                .truths
                .into_iter()
                .map(|(key, value)| (name.clone(), key, value)),
        );
        entities.push(crate::model::EntityInput {
            name,
            entity_type: entity.entity_type,
            observations: entity.observations,
        });
    }

    if !entities.is_empty() {
        store.create_entities(entities)?;
        for (name, key, value) in truths {
            store.truth_upsert(&name, &key, &value)?;
        }
    }
    if !graph.relations.is_empty() {
        store.create_relations(graph.relations)?;
    }
    Ok(())
}

fn print_init_report(report: &crate::init::InitReport) {
    let label = match report.target {
        crate::init::InitTarget::Xdg => "Initialised Asobi workspace (XDG)",
        crate::init::InitTarget::Local => "Initialised Asobi workspace (project-local)",
    };
    println!("{}", label);
    for dir in &report.created_dirs {
        println!("  created  {}", dir.display());
    }
    for dir in &report.skipped_dirs {
        println!("  exists   {}", dir.display());
    }
    if let Some(path) = &report.wrote_config {
        println!("  wrote    {}", path.display());
    } else if let Some(path) = &report.config_existed {
        println!("  exists   {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::import_graph;
    use crate::api::{GraphStore, MaintenanceStore};
    use crate::cli::runtime::validate_git_url;
    use crate::storage::Storage;
    use tempfile::tempdir;

    #[test]
    fn git_url_validator_rejects_option_and_command_urls() {
        assert!(validate_git_url("-upload-pack=x").is_err());
        assert!(validate_git_url("ext::sh -c id").is_err());
    }

    #[test]
    fn git_url_validator_accepts_supported_urls() {
        for url in [
            "https://example.com/repo.git",
            "ssh://example.com/repo.git",
            "git://example.com/repo.git",
            "file:///tmp/repo",
            "git@example.com:repo.git",
        ] {
            assert!(validate_git_url(url).is_ok(), "expected valid URL: {url}");
        }
    }

    #[test]
    fn import_graph_round_trips_truths() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                crate::paths::ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let backend = Storage::open_default().unwrap();
        backend
            .create_entities(vec![crate::model::EntityInput {
                name: "project".to_string(),
                entity_type: "task".to_string(),
                observations: vec!["ship it".to_string()],
            }])
            .unwrap();
        backend.truth_upsert("project", "status", "READY").unwrap();

        let exported = backend.read_graph_full().unwrap();
        backend.reset().unwrap();
        import_graph(&backend, exported).unwrap();

        let imported = backend.read_graph_full().unwrap();
        assert_eq!(
            imported.entities[0].truths.get("status"),
            Some(&"READY".to_string())
        );
    }
}
