use super::commands::{Cli, Commands};
use super::output::*;
use crate::api::{MaintenanceStore, PurgeRequest};
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
    let backend = runtime.storage();

    let json = cli.json;
    match cli.command {
        Commands::Compact {} => {
            let synced = crate::compact::sync_graph_to_markdown(backend)?;
            info!("Done. Synced {} entities to Markdown.", synced);
        }
        Commands::Purge { older_than, apply } => {
            let report = backend.purge(PurgeRequest {
                older_than_days: older_than,
                apply,
            })?;
            if json {
                print_json(report)?;
            } else {
                let action = if report.dry_run {
                    "would purge"
                } else {
                    "purged"
                };
                println!(
                    "Purge {}: {} candidate(s), {} entity/entities {}.",
                    if report.dry_run {
                        "preview"
                    } else {
                        "complete"
                    },
                    report.candidates.len(),
                    report.deleted,
                    action
                );
                for candidate in &report.candidates {
                    println!(
                        "  {} [{} / {}] last activity {} · {} observations · {} relations",
                        candidate.name,
                        candidate.entity_type,
                        candidate.status,
                        candidate.last_activity,
                        candidate.observations,
                        candidate.relations
                    );
                }
                if report.dry_run && !report.candidates.is_empty() {
                    println!("Re-run with --apply to delete these entities.");
                }
            }
        }
        Commands::Tasks { subcommand } => crate::tasks::run(backend, subcommand, json)?,
        Commands::Skills { subcommand } => super::skills::run(&paths, subcommand)?,
        command => super::graph::run(backend, command, json)?,
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
    use crate::api::MaintenanceStore;
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
}
