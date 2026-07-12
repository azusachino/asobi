use anyhow::Result;
use asobi::api::{
    BackupStore, GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore, SkillStore,
    Stats,
};
use asobi::application::AsobiRuntime;
use asobi::paths::AsobiPaths;
use clap::{Parser, Subcommand};
use schemars::JsonSchema;
use serde::Serialize;
#[cfg(feature = "documents")]
use std::sync::Arc;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

/// Timestamp logs in the machine's local timezone instead of tracing's default
/// UTC. Reuses the `chrono` dependency (already pulled in for backup/compact) so
/// this adds no crates and avoids tracing-subscriber's `local-time` feature,
/// whose `time`-crate offset lookup is unsound in a multithreaded process.
#[derive(Debug, Clone, Copy, Default)]
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct DeletedReceipt {
    deleted: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct EntityStatsDetail {
    name: String,
    observation_count: usize,
    limit: usize,
    percentage: f64,
    critical: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct StatsReceipt {
    entities: usize,
    relations: usize,
    observations: usize,
    database_path: &'static str,
    journal_mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    entities_detailed: Option<Vec<EntityStatsDetail>>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesReceipt {
    api_version: u32,
    capabilities: asobi::api::BackendCapabilities,
    health: asobi::api::BackendHealth,
}

#[derive(Parser)]
#[command(name = "asobi")]
#[command(version)]
#[command(about = "Asobi: Knowledge Graph & Memory CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// After a mutation, print the affected entity/entities as JSON to stdout
    /// (instead of leaving stdout empty). No effect on read commands, which
    /// already emit JSON.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest a file or directory into the document tier
    #[cfg(feature = "documents")]
    Ingest {
        /// Path to file or directory
        path: String,
    },
    /// Query topics or chunks using hybrid semantic + keyword search
    #[cfg(feature = "documents")]
    Query {
        /// Query string
        query: String,
        /// Maximum number of matched topics to return
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Print results as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create new entities in the knowledge graph.
    ///
    /// Accepts one or more `NAME TYPE` pairs:
    /// `new A task B concept` creates two entities.
    New {
        /// Flat list of `NAME TYPE` pairs (count must be a multiple of 2)
        #[arg(num_args = 2.., value_names = ["NAME", "TYPE"])]
        pairs: Vec<String>,
        /// Seed observations at creation: `--obs VALUE` (repeatable)
        #[arg(long = "obs", value_name = "OBSERVATION")]
        observations: Vec<String>,
    },
    /// Create relations between entities.
    ///
    /// Accepts one or more `FROM TO TYPE` triples:
    /// `link A B uses C D blocks` creates two relations.
    Link {
        /// Flat list of `FROM TO TYPE` triples (count must be a multiple of 3)
        #[arg(num_args = 3.., value_names = ["FROM", "TO", "TYPE"])]
        triples: Vec<String>,
    },
    /// Add observations to existing entities
    Obs {
        name: String,
        #[arg(num_args = 1..)]
        contents: Vec<String>,
    },
    /// Add or update a truth for an entity
    Truth {
        name: String,
        key: String,
        value: String,
    },
    /// Delete a specific truth for an entity
    RmTruth { name: String, key: String },
    /// Show an entity's truth change history (superseded values with validity windows)
    History {
        name: String,
        /// Limit to a single truth key
        key: Option<String>,
    },
    /// Delete entities and their relations
    Rm { names: Vec<String> },
    /// Delete specific observations
    RmObs {
        name: String,
        content: String,
        /// Match by observation ID instead of content
        #[arg(long)]
        id: bool,
    },
    /// Update an existing observation atomically (replaces old content with new content)
    UpdateObs {
        name: String,
        old_content: String,
        new_content: String,
        /// Match by observation ID instead of content
        #[arg(long)]
        id: bool,
    },
    /// Delete specific relations
    Unlink {
        from: String,
        to: String,
        relation_type: String,
    },
    /// Read the entire knowledge graph
    Graph,
    /// Search for nodes
    Search {
        /// Search query terms
        query: Option<String>,
        /// Maximum number of matched nodes to return
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Filter by entity truths: `--where KEY=VALUE` (repeatable)
        #[arg(long = "where", value_name = "KEY=VALUE")]
        filters: Vec<String>,
    },
    /// Retrieve specific nodes by name
    Show {
        names: Vec<String>,
        /// Expand relations of specified type(s) to include linked entities
        #[arg(long, value_name = "RELATION_TYPE")]
        expand: Vec<String>,
        /// Include observation IDs in detailed list
        #[arg(long)]
        with_ids: bool,
    },
    /// Report near-duplicate topics, prune sessions, and sync Graph to MD
    #[cfg(feature = "documents")]
    Compact {
        /// Prune sessions older than N days
        #[arg(long, default_value = "90")]
        older_than: u32,
    },
    /// Initialise a Asobi workspace (XDG by default, `--local` for cwd)
    Init {
        /// Create `.asobi/` and `asobi.toml` in the current directory
        /// instead of the user-level XDG paths.
        #[arg(long)]
        local: bool,
    },
    /// Show statistics about the knowledge graph
    Stats {
        /// Show observation counts and limits per entity
        #[arg(long)]
        per_entity: bool,
    },
    /// Report the API contract and selected backend capabilities
    Capabilities,
    /// Emit JSON Schema for command payloads
    Schema {
        /// Restrict output to one command's response schema
        #[arg(long)]
        command: Option<String>,
    },

    /// Export the knowledge graph to a JSON file
    Export {
        /// Path to the output JSON file
        #[arg(short, long)]
        output: Option<String>,
        /// Restrict the export to the subgraph rooted at these entities
        /// (repeatable). Pulls each root, its `part_of` children (transitively),
        /// and the `depends_on` targets they cite. Omit to export the whole graph.
        #[arg(long)]
        scope: Vec<String>,
        /// With --scope, also follow one hop of `supersedes`/`extends` off the
        /// cited leaves (the rationale chain behind a decision).
        #[arg(long)]
        rationale: bool,
    },
    /// Import a knowledge graph from a JSON file
    Import {
        /// Path to the input JSON file
        file: String,
    },
    /// Reset the knowledge graph (delete all entities, relations, and observations)
    Reset {
        /// Force reset without confirmation
        #[arg(long)]
        force: bool,
    },
    /// Snapshot the database to a single consistent file (VACUUM INTO)
    Backup {
        /// Destination path (default: `<data_dir>/backups/asobi-<timestamp>.db`)
        #[arg(short, long)]
        output: Option<String>,
        /// Snapshots to retain in the default backup directory (oldest pruned)
        #[arg(long, default_value_t = 3)]
        keep: usize,
    },
    /// Replace the live database with a snapshot file
    Restore {
        /// Path to the snapshot file to restore from
        file: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Manage, install, and update AI agent skills
    Skills {
        #[command(subcommand)]
        subcommand: Option<SkillsCommands>,
    },
}

#[derive(Subcommand, Debug)]
enum SkillsCommands {
    /// Install skills from a git repository or local path
    Install {
        /// Git URL or local directory path
        source: String,
        /// Install all skills found
        #[arg(long)]
        all: bool,
        /// Install specific skills by name
        #[arg(long, num_args = 1..)]
        select: Option<Vec<String>>,
    },
    /// Update installed skills from their sources
    Update {
        /// Specific source URL or slug to update (updates all if omitted)
        source: Option<String>,
    },
    /// Remove an installed skill or all skills from a source
    Remove {
        /// Name of the skill (e.g. skill:slug:name) or source slug/URL
        target: String,
    },
    /// Show the raw body of an installed skill (useful for humans to read without JSON escaping)
    Show {
        /// Name of the skill (fully qualified e.g. skill:slug:name, or short name)
        name: String,
    },
}

#[cfg(feature = "documents")]
fn needs_vector(cmd: &Commands) -> bool {
    matches!(
        cmd,
        Commands::Ingest { .. } | Commands::Query { .. } | Commands::Compact { .. }
    )
}

#[cfg(not(feature = "documents"))]
fn needs_vector(_: &Commands) -> bool {
    false
}

pub const ENV_FASTEMBED_CACHE_DIR: &str = "ASOBI_FASTEMBED_CACHE_DIR";
pub const ENV_TOPICS_DIR: &str = "ASOBI_TOPICS_DIR";

#[cfg(feature = "documents")]
fn init_embedder(paths: &AsobiPaths) -> Result<Arc<asobi::embed::FastEmbedProvider>> {
    let cache_dir = std::env::var(ENV_FASTEMBED_CACHE_DIR)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| paths.data_dir.join("fastembed_cache"));
    let embedder: Arc<asobi::embed::FastEmbedProvider> =
        Arc::new(asobi::embed::FastEmbedProvider::new(cache_dir)?);
    Ok(embedder)
}

/// Initialise the global tracing subscriber. Logs go to **stderr** so the
/// stdout channel stays clean for machine-readable data (graph JSON, stats) and
/// the MCP JSON-RPC stream. Level is controlled by `RUST_LOG` (default `info`).
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_timer(LocalTimer)
        .compact()
        .init();
}

/// Verify the `git` binary is reachable before any remote operation, so a
/// missing git yields a clear message instead of a raw `os error 2` from `?`.
fn ensure_git_available() -> Result<()> {
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => anyhow::bail!("`git --version` failed; ensure git is installed and on PATH"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "`git` not found on PATH — install git to install or update skills from a remote repository"
        ),
        Err(e) => anyhow::bail!("failed to invoke git: {e}"),
    }
}

fn validate_git_url(git_url: &str) -> Result<()> {
    if git_url.starts_with('-') {
        anyhow::bail!("Invalid git URL: URLs must not start with '-'");
    }

    let has_allowed_scheme = ["https://", "ssh://", "git://", "file://"]
        .iter()
        .any(|scheme| git_url.starts_with(scheme));
    let is_scp_style = git_url.starts_with("git@") && git_url.contains(':');
    if !has_allowed_scheme && !is_scp_style {
        anyhow::bail!(
            "Unsupported git URL '{}'; use https://, ssh://, git://, file://, or git@host:path",
            git_url
        );
    }

    Ok(())
}

fn get_or_update_cached_repo(
    git_url: &str,
    caches_dir: &std::path::Path,
) -> Result<(std::path::PathBuf, String)> {
    ensure_git_available()?;
    validate_git_url(git_url)?;
    let slug = asobi::skills::derive_source_slug(git_url);
    let repo_cache_dir = caches_dir.join(&slug);

    std::fs::create_dir_all(caches_dir)?;

    if repo_cache_dir.exists() {
        info!("Updating cached repository in {:?}...", repo_cache_dir);
        let fetch_status = std::process::Command::new("git")
            .arg("fetch")
            .arg("--depth")
            .arg("1")
            .current_dir(&repo_cache_dir)
            .status();

        let mut success = false;
        if let Ok(status) = fetch_status
            && status.success()
        {
            let reset_status = std::process::Command::new("git")
                .arg("reset")
                .arg("--hard")
                .arg("origin/HEAD")
                .current_dir(&repo_cache_dir)
                .status();
            if let Ok(status) = reset_status
                && status.success()
            {
                success = true;
            }
        }

        if !success {
            info!(
                "Failed to update existing cache, re-cloning to {:?}...",
                repo_cache_dir
            );
            let _ = std::fs::remove_dir_all(&repo_cache_dir);
            let clone_status = std::process::Command::new("git")
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg("--")
                .arg(git_url)
                .arg(&repo_cache_dir)
                .status()?;
            if !clone_status.success() {
                anyhow::bail!("Failed to clone repository from {}", git_url);
            }
        }
    } else {
        info!("Cloning {} to {:?}...", git_url, repo_cache_dir);
        let clone_status = std::process::Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--")
            .arg(git_url)
            .arg(&repo_cache_dir)
            .status()?;
        if !clone_status.success() {
            anyhow::bail!("Failed to clone repository from {}", git_url);
        }
    }

    let output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&repo_cache_dir)
        .output()?;
    let version = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        "unknown".to_string()
    };

    Ok((repo_cache_dir, version))
}

#[tokio::main]
async fn main() {
    init_tracing();
    let cli = Cli::parse();
    let json = cli.json;

    if let Err(e) = run_cli(cli).await {
        if json {
            let error_json = serde_json::json!({
                "status": "failed",
                "error": e.to_string()
            });
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
        } else {
            error!("{e:?}");
        }
        std::process::exit(1);
    }
}

async fn run_cli(cli: Cli) -> Result<()> {
    if let Commands::Schema { command } = cli.command {
        emit_schema(command.as_deref())?;
        return Ok(());
    }

    // `init` is special: it runs before any DB or config resolution, since
    // its job is to create the workspace those subsystems need.
    if let Commands::Init { local } = cli.command {
        let cwd = std::env::current_dir()?;
        let target = if local {
            asobi::init::InitTarget::Local
        } else {
            asobi::init::InitTarget::Xdg
        };
        let report = asobi::init::init_workspace(target, &cwd)?;
        print_init_report(&report);
        return Ok(());
    }

    let paths = AsobiPaths::resolve();
    let runtime = AsobiRuntime::open_default().await?;
    if let Commands::Restore { ref file, force } = cli.command {
        runtime
            .into_storage()
            .restore(std::path::PathBuf::from(file), force)
            .await?;
        return Ok(());
    }
    let backend = runtime.storage();

    // Vector store + embedder are only initialised for commands that need them.
    // Graph-only operations (new, graph, etc.) skip the heavy
    // fastembed model load entirely.
    if needs_vector(&cli.command) {
        #[cfg(feature = "documents")]
        {
            let embedder = init_embedder(&paths)?;
            match cli.command {
                Commands::Ingest { path } => {
                    let p = std::path::Path::new(&path);
                    if p.is_dir() {
                        info!("Ingesting directory: {:?}...", p);
                        let count =
                            asobi::ingest::ingest_dir(p, backend, embedder.as_ref()).await?;
                        info!("Done. Ingested {} files.", count);
                    } else {
                        info!("Ingesting file: {:?}...", p);
                        asobi::ingest::ingest_file(p, backend, embedder.as_ref()).await?;
                        info!("Done.");
                    }
                }
                Commands::Query { query, limit, json } => {
                    info!("Searching: {}...", query);
                    let results =
                        asobi::recall::recall(&query, backend, embedder.as_ref(), limit).await?;
                    if json {
                        print_json(results)?;
                    } else if results.is_empty() {
                        info!("No results found.");
                    } else {
                        for r in results {
                            println!(
                                "{:<20} | (score: {:.2}) | {}",
                                r.title, r.score, r.file_path
                            );
                        }
                    }
                }
                Commands::Compact { older_than } => {
                    let topics_root = std::env::var(ENV_TOPICS_DIR)
                        .unwrap_or_else(|_| paths.topics_dir.to_str().unwrap().to_string());
                    let pruned = asobi::compact::prune_old_sessions(&topics_root, older_than)?;
                    info!("Pruned {} old session files.", pruned);

                    let clusters = asobi::compact::find_duplicate_clusters(backend, 0.85).await?;
                    info!("Found {} near-duplicate topic clusters.", clusters.len());
                    for (i, cluster) in clusters.iter().enumerate() {
                        info!("  Cluster {}: {}", i + 1, cluster.join(", "));
                    }

                    info!("Syncing Graph to Markdown...");
                    let synced =
                        asobi::compact::sync_graph_to_markdown(backend, backend, embedder.as_ref())
                            .await?;
                    info!("Done. Synced {} entities to Markdown.", synced);
                }
                _ => unreachable!(),
            }
        }
        return Ok(());
    }

    let json = cli.json;
    match cli.command {
        Commands::New {
            pairs,
            observations,
        } => {
            if pairs.is_empty() || pairs.len() % 2 != 0 {
                anyhow::bail!(
                    "new expects one or more `NAME TYPE` pairs (got {} arguments)",
                    pairs.len()
                );
            }
            let entities: Vec<asobi::model::EntityInput> = pairs
                .chunks_exact(2)
                .map(|c| asobi::model::EntityInput {
                    name: c[0].clone(),
                    entity_type: c[1].clone(),
                    observations: observations.clone(),
                })
                .collect();
            let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
            backend.create_entities(entities).await?;
            info!("{} entit{} created.", names.len(), plural(names.len()));
            if json {
                emit_nodes(backend, names).await?;
            }
        }
        Commands::Link { triples } => {
            if triples.is_empty() || triples.len() % 3 != 0 {
                anyhow::bail!(
                    "link expects one or more `FROM TO TYPE` triples (got {} arguments)",
                    triples.len()
                );
            }
            let relations: Vec<asobi::model::RelationInput> = triples
                .chunks_exact(3)
                .map(|c| asobi::model::RelationInput {
                    from: c[0].clone(),
                    to: c[1].clone(),
                    relation_type: c[2].clone(),
                })
                .collect();
            let involved: Vec<String> = relations
                .iter()
                .flat_map(|r| [r.from.clone(), r.to.clone()])
                .collect();
            let count = relations.len();
            backend.create_relations(relations).await?;
            info!("{} relation{} created.", count, suffix(count));
            if json {
                emit_nodes(backend, involved).await?;
            }
        }
        Commands::Obs { name, contents } => {
            let paths = asobi::paths::AsobiPaths::resolve();
            let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(paths.observation_limit.unwrap_or(200));
            backend
                .add_observations(
                    vec![asobi::model::ObservationInput {
                        entity_name: name.clone(),
                        contents,
                    }],
                    limit,
                )
                .await?;
            info!("Observation added.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::Truth { name, key, value } => {
            backend.truth_upsert(&name, &key, &value).await?;
            info!("Truth added.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::RmTruth { name, key } => {
            backend.truth_delete(&name, &key).await?;
            info!("Truth deleted.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::History { name, key } => {
            let history = backend.truth_history(&name, key.as_deref()).await?;
            print_json(history)?;
        }
        Commands::Rm { names } => {
            let deleted = names.clone();
            backend.delete_entities(names).await?;
            info!("Entities deleted.");
            if json {
                print_json(DeletedReceipt { deleted })?;
            }
        }
        Commands::RmObs { name, content, id } => {
            if id {
                let parsed_id = content.parse::<i64>().map_err(|_| {
                    anyhow::anyhow!(
                        "Invalid observation ID: '{}'. Expected an integer.",
                        content
                    )
                })?;
                backend.delete_observation_by_id(&name, parsed_id).await?;
            } else {
                backend
                    .delete_observations(vec![asobi::model::ObservationDeletion {
                        entity_name: name.clone(),
                        observations: vec![content],
                    }])
                    .await?;
            }
            info!("Observations deleted.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::UpdateObs {
            name,
            old_content,
            new_content,
            id,
        } => {
            if id {
                let parsed_id = old_content.parse::<i64>().map_err(|_| {
                    anyhow::anyhow!(
                        "Invalid observation ID: '{}'. Expected an integer.",
                        old_content
                    )
                })?;
                backend
                    .update_observation_by_id(&name, parsed_id, &new_content)
                    .await?;
            } else {
                backend
                    .update_observation(&name, &old_content, &new_content)
                    .await?;
            }
            info!("Observation updated.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::Unlink {
            from,
            to,
            relation_type,
        } => {
            backend
                .delete_relations(vec![asobi::model::RelationInput {
                    from: from.clone(),
                    to: to.clone(),
                    relation_type,
                }])
                .await?;
            info!("Relations deleted.");
            if json {
                emit_nodes(backend, vec![from, to]).await?;
            }
        }
        Commands::Graph => {
            let graph = backend.read_graph().await?;
            print_json(graph)?;
        }
        Commands::Search {
            query,
            limit,
            filters,
        } => {
            let mut parsed_filters = Vec::new();
            for f in &filters {
                if let Some((k, v)) = f.split_once('=') {
                    parsed_filters.push((k.trim().to_string(), v.trim().to_string()));
                } else {
                    anyhow::bail!("Invalid filter format: '{}'. Expected KEY=VALUE.", f);
                }
            }
            let query_str = query.unwrap_or_default();
            let graph = backend
                .search_nodes(SearchQuery {
                    query: query_str,
                    limit,
                    filters: parsed_filters,
                })
                .await?;
            print_json(graph)?;
        }
        Commands::Show {
            names,
            expand,
            with_ids,
        } => {
            let graph = backend
                .open_nodes(OpenNodes {
                    names,
                    with_ids,
                    expand,
                })
                .await?;
            print_json(graph)?;
        }
        Commands::Stats { per_entity } => {
            let db_path = "provider-managed";
            let journal_mode = "provider-managed";

            let Stats {
                entities,
                relations,
                observations,
            } = backend.stats().await?;
            if json {
                let entities_detailed = if per_entity {
                    let paths = asobi::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity().await?;
                    Some(
                        list.iter()
                            .map(|(name, count)| {
                                let pct = if limit > 0 {
                                    (*count as f64 / limit as f64) * 100.0
                                } else {
                                    0.0
                                };
                                EntityStatsDetail {
                                    name: name.clone(),
                                    observation_count: *count,
                                    limit,
                                    percentage: pct,
                                    critical: limit > 0 && *count >= (limit * 80 / 100),
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                };

                print_json(StatsReceipt {
                    entities,
                    relations,
                    observations,
                    database_path: db_path,
                    journal_mode,
                    entities_detailed,
                })?;
            } else {
                println!("Database Path:  {}", db_path);
                println!("Journal Mode:   {}", journal_mode);
                println!("Knowledge Graph Statistics:");
                println!("  Entities:     {}", entities);
                println!("  Relations:    {}", relations);
                println!("  Observations: {}", observations);

                if per_entity {
                    let paths = asobi::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity().await?;
                    if !list.is_empty() {
                        println!("\nEntities by Observation Count:");
                        for (name, count) in &list {
                            let pct = if limit > 0 {
                                (*count as f64 / limit as f64) * 100.0
                            } else {
                                0.0
                            };
                            if limit > 0 && *count >= (limit * 80 / 100) {
                                println!(
                                    "  {:_<40} {} / {} (CRITICAL: {:.1}%)",
                                    name, count, limit, pct
                                );
                            } else {
                                println!("  {:_<40} {}", name, count);
                            }
                        }
                    }
                }
            }
        }
        Commands::Capabilities => {
            let capabilities = backend.capabilities().await?;
            let health = backend.health().await?;
            print_json(CapabilitiesReceipt {
                api_version: asobi::api::API_VERSION,
                capabilities,
                health,
            })?;
        }

        Commands::Export {
            output,
            scope,
            rationale,
        } => {
            let graph = if scope.is_empty() {
                backend.read_graph_full().await?
            } else {
                backend.read_graph_scoped(&scope, rationale).await?
            };
            if let Some(path) = output {
                let json = serde_json::to_string_pretty(&graph)?;
                std::fs::write(&path, json)?;
                asobi::application::restrict_permissions(std::path::Path::new(&path), 0o600)?;
                info!("Graph exported to {}", path);
            } else {
                print_json(graph)?;
            }
        }
        Commands::Import { file } => {
            let content = std::fs::read_to_string(&file)?;
            let graph: asobi::model::Graph = serde_json::from_str(&content)?;

            let had_entities = !graph.entities.is_empty();
            let had_relations = !graph.relations.is_empty();
            import_graph(backend, graph).await?;
            if had_entities {
                info!("Imported entities, observations, and truths.");
            }
            if had_relations {
                info!("Imported relations.");
            }
            info!("Import complete.");
        }
        Commands::Reset { force } => {
            if !force {
                use std::io::Write;
                print!("Are you sure you want to completely clear the knowledge graph? [y/N]: ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    info!("Reset aborted.");
                    return Ok(());
                }
            }
            backend.reset().await?;
            info!("Knowledge graph reset successfully.");
        }
        Commands::Backup { output, keep } => {
            let receipt = backend
                .backup(asobi::api::v1::BackupRequest {
                    destination: output.map(std::path::PathBuf::from).unwrap_or_default(),
                    keep,
                })
                .await?;
            info!("Backup written to {}", receipt.path.display());
        }
        Commands::Restore { .. } => unreachable!("restore handled before borrowing storage"),
        Commands::Skills { subcommand } => {
            use std::io::IsTerminal;
            match subcommand {
                None => {
                    let skills = backend.list_skills().await?;
                    if skills.is_empty() {
                        println!("No skills installed.");
                    } else {
                        let mut grouped: std::collections::BTreeMap<
                            String,
                            Vec<asobi::api::v1::SkillRecord>,
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
                        let (cache_path, ver) =
                            get_or_update_cached_repo(&git_url, &paths.caches_dir())?;
                        (cache_path, ver)
                    } else {
                        let local_path = std::path::Path::new(&source);
                        if !local_path.exists() {
                            anyhow::bail!("Local path {} does not exist", source);
                        }
                        (local_path.to_path_buf(), "local".to_string())
                    };

                    let mode = if all {
                        asobi::skills::SelectionMode::All
                    } else if let Some(sel) = select {
                        asobi::skills::SelectionMode::Select(sel)
                    } else {
                        asobi::skills::SelectionMode::Interactive
                    };

                    let is_tty = std::io::stdin().is_terminal();

                    #[cfg(feature = "documents")]
                    let embedder = init_embedder(&paths)?;

                    // `--all` is a full sync of the source: prune skills that
                    // vanished upstream. `--select` / interactive stay additive.
                    let prune = matches!(mode, asobi::skills::SelectionMode::All);

                    #[cfg(feature = "documents")]
                    asobi::skills::install_skills_from_store(
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
                    asobi::skills::install_skills_from_dir(
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
                    let embedder = init_embedder(&paths)?;

                    let skills = backend.list_skills().await?;
                    let mut unique_sources = std::collections::HashSet::new();
                    for s in skills {
                        if let Some(ref filter_src) = source {
                            let slug = asobi::skills::derive_source_slug(&s.source);
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
                        asobi::skills::install_skills_from_store(
                            backend,
                            backend,
                            embedder.as_ref(),
                            &target_path,
                            &git_url,
                            &version,
                            asobi::skills::SelectionMode::All,
                            false,
                            true,
                        )
                        .await?;
                        #[cfg(not(feature = "documents"))]
                        asobi::skills::install_skills_from_dir(
                            backend,
                            &target_path,
                            &git_url,
                            &version,
                            asobi::skills::SelectionMode::All,
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
                        let slug = asobi::skills::derive_source_slug(&s.source);
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
                                s.entity_name == name
                                    || s.entity_name.ends_with(&format!(":{}", name))
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
        }
        _ => unreachable!(),
    }

    Ok(())
}

/// Print the named entities (and the relations among them) as pretty JSON to
/// stdout — the `--json` echo after a mutation, so a caller can confirm the
/// write without a second `show` round-trip. Names are normalized inside
/// `open_nodes`, so raw user input matches what was just stored.
async fn emit_nodes(store: &impl GraphStore, names: Vec<String>) -> Result<()> {
    let graph = store
        .open_nodes(OpenNodes {
            names,
            ..Default::default()
        })
        .await?;
    print_json(graph)?;
    Ok(())
}

fn print_json<T: Serialize>(value: T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

const CLI_SCHEMA_VERSION: u32 = 1;

fn schema_for_data<T: JsonSchema>() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(T))
        .expect("response schemas must serialize to JSON");
    schema["x-asobi-schema-version"] = serde_json::json!(CLI_SCHEMA_VERSION);
    schema
}

/// A command name paired with a builder for its payload's JSON Schema.
type SchemaRow = (&'static str, fn() -> serde_json::Value);

/// The one source of truth mapping a command to the JSON Schema of its
/// machine-readable payload. Most commands echo the affected `Graph`; the rest
/// have their own receipt type. Adding a command means adding one row here.
fn schema_registry() -> Vec<SchemaRow> {
    use asobi::model::Graph;
    let rows: Vec<SchemaRow> = vec![
        ("capabilities", schema_for_data::<CapabilitiesReceipt>),
        ("export", schema_for_data::<Graph>),
        ("graph", schema_for_data::<Graph>),
        (
            "history",
            schema_for_data::<Vec<asobi::api::v1::TruthVersion>>,
        ),
        ("link", schema_for_data::<Graph>),
        ("new", schema_for_data::<Graph>),
        ("obs", schema_for_data::<Graph>),
        ("rm", schema_for_data::<DeletedReceipt>),
        ("rm-obs", schema_for_data::<Graph>),
        ("rm-truth", schema_for_data::<Graph>),
        ("search", schema_for_data::<Graph>),
        ("show", schema_for_data::<Graph>),
        ("stats", schema_for_data::<StatsReceipt>),
        ("truth", schema_for_data::<Graph>),
        ("unlink", schema_for_data::<Graph>),
        ("update-obs", schema_for_data::<Graph>),
    ];
    #[cfg(feature = "documents")]
    let rows = {
        let mut rows = rows;
        rows.push(("query", schema_for_data::<Vec<asobi::recall::RecallResult>>));
        rows
    };
    rows
}

fn emit_schema(command: Option<&str>) -> Result<()> {
    let registry = schema_registry();
    if let Some(command) = command {
        let (_, schema_of) = registry
            .iter()
            .find(|(name, _)| *name == command)
            .ok_or_else(|| anyhow::anyhow!("unknown schema command: {command}"))?;
        print_json(schema_of())
    } else {
        let commands: std::collections::BTreeMap<_, _> = registry
            .iter()
            .map(|(name, schema_of)| (*name, schema_of()))
            .collect();
        print_json(serde_json::json!({
            "schemaVersion": CLI_SCHEMA_VERSION,
            "commands": commands,
        }))
    }
}

async fn import_graph(store: &impl GraphStore, graph: asobi::model::Graph) -> Result<()> {
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
        entities.push(asobi::model::EntityInput {
            name,
            entity_type: entity.entity_type,
            observations: entity.observations,
        });
    }

    if !entities.is_empty() {
        store.create_entities(entities).await?;
        for (name, key, value) in truths {
            store.truth_upsert(&name, &key, &value).await?;
        }
    }
    if !graph.relations.is_empty() {
        store.create_relations(graph.relations).await?;
    }
    Ok(())
}

/// `""`/`"s"` suffix for count-based log lines.
fn suffix(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `"y"`/`"ies"` suffix for "entit{y,ies}".
fn plural(n: usize) -> &'static str {
    if n == 1 { "y" } else { "ies" }
}

fn print_init_report(report: &asobi::init::InitReport) {
    let label = match report.target {
        asobi::init::InitTarget::Xdg => "Initialised Asobi workspace (XDG)",
        asobi::init::InitTarget::Local => "Initialised Asobi workspace (project-local)",
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
    use super::{import_graph, validate_git_url};
    use asobi::api::{GraphStore, MaintenanceStore};
    use asobi::storage::Storage;
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

    #[tokio::test]
    async fn import_graph_round_trips_truths() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                asobi::paths::ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let backend = Storage::open_default().await.unwrap();
        backend
            .create_entities(vec![asobi::model::EntityInput {
                name: "project".to_string(),
                entity_type: "task".to_string(),
                observations: vec!["ship it".to_string()],
            }])
            .await
            .unwrap();
        backend
            .truth_upsert("project", "status", "READY")
            .await
            .unwrap();

        let exported = backend.read_graph_full().await.unwrap();
        backend.reset().await.unwrap();
        import_graph(&backend, exported).await.unwrap();

        let imported = backend.read_graph_full().await.unwrap();
        assert_eq!(
            imported.entities[0].truths.get("status"),
            Some(&"READY".to_string())
        );
    }
}
