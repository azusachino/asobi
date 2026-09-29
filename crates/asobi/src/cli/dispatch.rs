use super::commands::{Cli, Commands};
use super::output::*;
use crate::application::AsobiRuntime;
use anyhow::Result;
use asobi_core::api::{MaintenanceStore, PurgeRequest};
use clap::CommandFactory;
use tracing::info;

pub(crate) async fn run_cli(cli: Cli) -> Result<()> {
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

    if let Commands::Version = cli.command {
        let remote = !cli.local_graph
            && crate::config::resolve(&asobi_core::paths::AsobiPaths::resolve())
                .remote
                .is_some();
        let server = if remote {
            AsobiRuntime::open_default()
                .await?
                .storage()
                .location()
                .await?
                .server_version
                .unwrap_or_else(|| "unknown".into())
        } else {
            "not applicable".into()
        };
        if cli.json {
            print_json(VersionReceipt {
                client_version: env!("CARGO_PKG_VERSION").into(),
                server_version: server,
                api_version: asobi_core::api::API_VERSION,
            })?;
        } else {
            println!("Client: {}", env!("CARGO_PKG_VERSION"));
            println!("Server: {server}");
            println!("API:    v{}", asobi_core::api::API_VERSION);
        }
        return Ok(());
    }
    let runtime = if cli.local_graph {
        AsobiRuntime::open_local().await?
    } else {
        AsobiRuntime::open_default().await?
    };
    let backend = runtime.storage();
    let json = cli.json;
    match cli.command {
        Commands::Compact {} => {
            let synced = crate::compact::sync_graph_to_markdown(backend).await?;
            info!("Done. Synced {} entities to Markdown.", synced);
        }
        Commands::Purge { older_than, apply } => {
            let report = backend
                .purge(PurgeRequest {
                    older_than_days: older_than,
                    apply,
                })
                .await?;
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
        Commands::Tasks { subcommand } => crate::tasks::run(backend, subcommand, json).await?,
        command => super::graph::run(backend, command, json).await?,
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
