use anyhow::Result;
use clap::{Parser, Subcommand};
#[cfg(feature = "documents")]
use rosemary::embed::EmbeddingProvider;
#[cfg(feature = "documents")]
use rosemary::paths::RosemaryPaths;
#[cfg(feature = "documents")]
use std::path::Path;
#[cfg(feature = "documents")]
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "rosemary")]
#[command(version)]
#[command(about = "Rosemary: Knowledge Graph & Memory CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    },
    /// Create new entities in the knowledge graph
    CreateEntities { name: String, entity_type: String },
    /// Create relations between entities
    CreateRelations {
        from: String,
        to: String,
        relation_type: String,
    },
    /// Add observations to existing entities
    AddObservations {
        name: String,
        #[arg(num_args = 1..)]
        contents: Vec<String>,
    },
    /// Add or update a truth for an entity
    AddTruth {
        name: String,
        key: String,
        value: String,
    },
    /// Delete a specific truth for an entity
    DeleteTruth { name: String, key: String },
    /// Delete entities and their relations
    DeleteEntities { names: Vec<String> },
    /// Delete specific observations
    DeleteObservations { name: String, content: String },
    /// Delete specific relations
    DeleteRelations {
        from: String,
        to: String,
        relation_type: String,
    },
    /// Read the entire knowledge graph
    ReadGraph,
    /// Search for nodes
    SearchNodes {
        query: String,
        /// Maximum number of matched nodes to return
        #[arg(long, default_value_t = rosemary::db::DEFAULT_SEARCH_LIMIT)]
        limit: usize,
    },
    /// Retrieve specific nodes by name
    OpenNodes { names: Vec<String> },
    /// Merge near-duplicate topics, prune sessions, and sync Graph to MD
    #[cfg(feature = "documents")]
    Compact {
        /// Prune sessions older than N days
        #[arg(long, default_value = "90")]
        older_than: u32,
    },
    /// Start the MCP stdio server (legacy/compatibility)
    Mcp,
    /// Initialise a Rosemary workspace (XDG by default, `--local` for cwd)
    Init {
        /// Create `.rosemary/` and `rosemary.toml` in the current directory
        /// instead of the user-level XDG paths.
        #[arg(long)]
        local: bool,
    },
    /// Show statistics about the knowledge graph
    Stats,
    /// Export the knowledge graph to a JSON file
    Export {
        /// Path to the output JSON file
        #[arg(short, long)]
        output: Option<String>,
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
        /// Destination path (default: `<data_dir>/backups/rosemary-<timestamp>.db`)
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

pub const ENV_FASTEMBED_CACHE_DIR: &str = "ROSEMARY_FASTEMBED_CACHE_DIR";
pub const ENV_EMBED_PROVIDER: &str = "ROSEMARY_EMBED_PROVIDER";
pub const ENV_TOPICS_DIR: &str = "ROSEMARY_TOPICS_DIR";

#[cfg(feature = "documents")]
async fn init_vector(
    conn: libsql::Connection,
    paths: &RosemaryPaths,
) -> Result<(
    rosemary::vector::VectorStore,
    Arc<rosemary::embed::FastEmbedProvider>,
)> {
    let store = rosemary::vector::VectorStore::new(conn);
    let embedder: Arc<rosemary::embed::FastEmbedProvider> =
        if std::env::var(ENV_EMBED_PROVIDER).as_deref() == Ok("claude") {
            anyhow::bail!("ClaudeProvider not yet implemented")
        } else {
            let cache_dir = std::env::var(ENV_FASTEMBED_CACHE_DIR)
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| paths.data_dir.join("fastembed_cache"));
            Arc::new(rosemary::embed::FastEmbedProvider::new(cache_dir)?)
        };
    if store.dim() != embedder.dim() {
        anyhow::bail!(
            "Vector store dimension mismatch: store={}, embedder={}",
            store.dim(),
            embedder.dim()
        );
    }
    Ok((store, embedder))
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
        .compact()
        .init();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();

    // `init` is special: it runs before any DB or config resolution, since
    // its job is to create the workspace those subsystems need.
    if let Commands::Init { local } = cli.command {
        let cwd = std::env::current_dir()?;
        let target = if local {
            rosemary::init::InitTarget::Local
        } else {
            rosemary::init::InitTarget::Xdg
        };
        let report = rosemary::init::init_workspace(target, &cwd)?;
        print_init_report(&report);
        return Ok(());
    }

    #[cfg(feature = "documents")]
    let paths = RosemaryPaths::resolve();
    let (db, conn) = rosemary::db::init_db().await?;

    // Vector store + embedder are only initialised for commands that need them.
    // Graph-only operations (create-entities, read-graph, etc.) skip the heavy
    // fastembed model load entirely.
    if needs_vector(&cli.command) {
        #[cfg(feature = "documents")]
        {
            let (store, embedder) = init_vector(conn, &paths).await?;
            match cli.command {
                Commands::Ingest { path } => {
                    let p = Path::new(&path);
                    if p.is_dir() {
                        info!("Ingesting directory: {:?}...", p);
                        let count = rosemary::ingest::ingest_dir(
                            p,
                            store.conn(),
                            &store,
                            embedder.as_ref(),
                        )
                        .await?;
                        info!("Done. Ingested {} files.", count);
                    } else {
                        info!("Ingesting file: {:?}...", p);
                        rosemary::ingest::ingest_file(p, store.conn(), &store, embedder.as_ref())
                            .await?;
                        info!("Done.");
                    }
                }
                Commands::Query { query } => {
                    info!("Searching: {}...", query);
                    let results = rosemary::recall::recall(
                        &query,
                        store.conn(),
                        &store,
                        embedder.as_ref(),
                        5,
                    )
                    .await?;
                    if results.is_empty() {
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
                    let pruned = rosemary::compact::prune_old_sessions(&topics_root, older_than)?;
                    info!("Pruned {} old session files.", pruned);

                    let clusters =
                        rosemary::compact::find_duplicate_clusters(&store, store.conn(), 0.85)
                            .await?;
                    info!("Found {} near-duplicate topic clusters.", clusters.len());

                    info!("Syncing Graph to Markdown...");
                    let synced = rosemary::compact::sync_graph_to_markdown(
                        store.conn(),
                        &store,
                        embedder.as_ref(),
                    )
                    .await?;
                    info!("Done. Synced {} entities to Markdown.", synced);
                }
                _ => unreachable!(),
            }
        }
        return Ok(());
    }

    match cli.command {
        Commands::CreateEntities { name, entity_type } => {
            rosemary::db::mcp_create_entities(
                &conn,
                vec![rosemary::mcp::EntityInput {
                    name: name.clone(),
                    entity_type,
                    observations: vec![],
                }],
            )
            .await?;
            info!("Entity '{}' created.", name);
        }
        Commands::CreateRelations {
            from,
            to,
            relation_type,
        } => {
            rosemary::db::mcp_create_relations(
                &conn,
                vec![rosemary::mcp::RelationInput {
                    from,
                    to,
                    relation_type,
                }],
            )
            .await?;
            info!("Relation created.");
        }
        Commands::AddObservations { name, contents } => {
            let paths = rosemary::paths::RosemaryPaths::resolve();
            let limit = std::env::var(rosemary::constant::ENV_OBSERVATION_LIMIT)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(paths.observation_limit.unwrap_or(50));
            rosemary::db::mcp_add_observations(
                &conn,
                vec![rosemary::mcp::ObservationInput {
                    entity_name: name,
                    contents,
                }],
                limit,
            )
            .await?;
            info!("Observation added.");
        }
        Commands::AddTruth { name, key, value } => {
            rosemary::db::truth_upsert(&conn, &name, &key, &value).await?;
            info!("Truth added.");
        }
        Commands::DeleteTruth { name, key } => {
            rosemary::db::truth_delete(&conn, &name, &key).await?;
            info!("Truth deleted.");
        }
        Commands::DeleteEntities { names } => {
            rosemary::db::mcp_delete_entities(&conn, names).await?;
            info!("Entities deleted.");
        }
        Commands::DeleteObservations { name, content } => {
            rosemary::db::mcp_delete_observations(
                &conn,
                vec![rosemary::mcp::ObservationDeletion {
                    entity_name: name,
                    observations: vec![content],
                }],
            )
            .await?;
            info!("Observations deleted.");
        }
        Commands::DeleteRelations {
            from,
            to,
            relation_type,
        } => {
            rosemary::db::mcp_delete_relations(
                &conn,
                vec![rosemary::mcp::RelationInput {
                    from,
                    to,
                    relation_type,
                }],
            )
            .await?;
            info!("Relations deleted.");
        }
        Commands::ReadGraph => {
            let graph = rosemary::db::mcp_read_graph(&conn).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::SearchNodes { query, limit } => {
            let graph = rosemary::db::mcp_search_nodes_with_limit(&conn, &query, limit).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::OpenNodes { names } => {
            let graph = rosemary::db::mcp_open_nodes(&conn, names).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::Mcp => {
            rosemary::mcp::run_server(conn).await?;
        }
        Commands::Stats => {
            let (entities, relations, observations) = rosemary::db::mcp_stats(&conn).await?;
            println!("Knowledge Graph Statistics:");
            println!("  Entities:     {}", entities);
            println!("  Relations:    {}", relations);
            println!("  Observations: {}", observations);
        }
        Commands::Export { output } => {
            let graph = rosemary::db::mcp_read_graph_eager(&conn).await?;
            let json = serde_json::to_string_pretty(&graph)?;
            if let Some(path) = output {
                std::fs::write(&path, json)?;
                info!("Graph exported to {}", path);
            } else {
                println!("{}", json);
            }
        }
        Commands::Import { file } => {
            let content = std::fs::read_to_string(&file)?;
            let graph: rosemary::mcp::Graph = serde_json::from_str(&content)?;

            // Re-construct entity inputs
            let mut entities = Vec::new();
            for e in graph.entities {
                entities.push(rosemary::mcp::EntityInput {
                    name: e.name,
                    entity_type: e.entity_type,
                    observations: e.observations,
                });
            }

            if !entities.is_empty() {
                rosemary::db::mcp_create_entities(&conn, entities).await?;
                info!("Imported entities and observations.");
            }
            if !graph.relations.is_empty() {
                rosemary::db::mcp_create_relations(&conn, graph.relations).await?;
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
            rosemary::db::mcp_reset(&conn).await?;
            info!("Knowledge graph reset successfully.");
        }
        Commands::Backup { output, keep } => {
            let dest =
                rosemary::backup::backup(&conn, output.map(std::path::PathBuf::from), keep).await?;
            info!("Backup written to {}", dest.display());
        }
        Commands::Restore { file, force } => {
            rosemary::backup::restore(db, conn, std::path::Path::new(&file), force).await?;
        }
        Commands::Skills { subcommand } => {
            use std::io::IsTerminal;
            match subcommand {
                None => {
                    let skills = rosemary::db::list_skills(&conn).await?;
                    if skills.is_empty() {
                        println!("No skills installed.");
                    } else {
                        let mut grouped: std::collections::BTreeMap<
                            String,
                            Vec<rosemary::db::SkillRow>,
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
                    let temp_dir = tempfile::tempdir()?;
                    let temp_path = temp_dir.path();
                    let is_git = source.contains("://")
                        || source.contains("git@")
                        || (!std::path::Path::new(&source).is_dir() && source.ends_with(".git"));

                    let version = if is_git {
                        info!("Cloning {}...", source);
                        let status = std::process::Command::new("git")
                            .arg("clone")
                            .arg("--depth")
                            .arg("1")
                            .arg(&source)
                            .arg(temp_path)
                            .status()?;
                        if !status.success() {
                            anyhow::bail!("Failed to clone repository from {}", source);
                        }
                        let output = std::process::Command::new("git")
                            .arg("rev-parse")
                            .arg("HEAD")
                            .current_dir(temp_path)
                            .output()?;
                        if output.status.success() {
                            String::from_utf8_lossy(&output.stdout).trim().to_string()
                        } else {
                            "unknown".to_string()
                        }
                    } else {
                        let local_path = std::path::Path::new(&source);
                        if !local_path.exists() {
                            anyhow::bail!("Local path {} does not exist", source);
                        }
                        "local".to_string()
                    };

                    let mode = if all {
                        rosemary::skills::SelectionMode::All
                    } else if let Some(sel) = select {
                        rosemary::skills::SelectionMode::Select(sel)
                    } else {
                        rosemary::skills::SelectionMode::Interactive
                    };

                    let is_tty = std::io::stdin().is_terminal();
                    let target_path = if is_git {
                        temp_path
                    } else {
                        std::path::Path::new(&source)
                    };

                    rosemary::skills::install_skills_from_dir(
                        &conn,
                        target_path,
                        &source,
                        &version,
                        mode,
                        is_tty,
                    )
                    .await?;
                    info!("Skills installed successfully.");
                }
                Some(SkillsCommands::Update { source }) => {
                    let skills = rosemary::db::list_skills(&conn).await?;
                    let mut unique_sources = std::collections::HashSet::new();
                    for s in skills {
                        if let Some(ref filter_src) = source {
                            let slug = rosemary::skills::derive_source_slug(&s.source);
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
                        let temp_dir = tempfile::tempdir()?;
                        let temp_path = temp_dir.path();
                        let is_git = src.contains("://")
                            || src.contains("git@")
                            || (!std::path::Path::new(&src).is_dir() && src.ends_with(".git"));

                        let version = if is_git {
                            let status = std::process::Command::new("git")
                                .arg("clone")
                                .arg("--depth")
                                .arg("1")
                                .arg(&src)
                                .arg(temp_path)
                                .status()?;
                            if !status.success() {
                                warn!("Failed to clone repository from {}", src);
                                continue;
                            }
                            let output = std::process::Command::new("git")
                                .arg("rev-parse")
                                .arg("HEAD")
                                .current_dir(temp_path)
                                .output()?;
                            if output.status.success() {
                                String::from_utf8_lossy(&output.stdout).trim().to_string()
                            } else {
                                "unknown".to_string()
                            }
                        } else {
                            "local".to_string()
                        };

                        let target_path = if is_git {
                            temp_path
                        } else {
                            std::path::Path::new(&src)
                        };

                        rosemary::skills::install_skills_from_dir(
                            &conn,
                            target_path,
                            &src,
                            &version,
                            rosemary::skills::SelectionMode::All,
                            false,
                        )
                        .await?;
                        info!("Successfully updated skills from {}.", src);
                    }
                }
                Some(SkillsCommands::Remove { target }) => {
                    let skills = rosemary::db::list_skills(&conn).await?;
                    let mut entities_to_delete = Vec::new();
                    for s in skills {
                        let slug = rosemary::skills::derive_source_slug(&s.source);
                        if s.entity_name == target || s.source == target || slug == target {
                            entities_to_delete.push(s.entity_name.clone());
                        }
                    }

                    if entities_to_delete.is_empty() {
                        if target.starts_with("skill:") {
                            entities_to_delete.push(target.clone());
                        } else {
                            anyhow::bail!("No installed skills found matching target {:?}", target);
                        }
                    }

                    info!("Deleting {} skill entities...", entities_to_delete.len());
                    rosemary::db::mcp_delete_entities(&conn, entities_to_delete).await?;
                    info!("Skills removed successfully.");
                }
            }
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn print_init_report(report: &rosemary::init::InitReport) {
    let label = match report.target {
        rosemary::init::InitTarget::Xdg => "Initialised Rosemary workspace (XDG)",
        rosemary::init::InitTarget::Local => "Initialised Rosemary workspace (project-local)",
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
