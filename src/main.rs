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
use tracing::info;
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
            rosemary::db::mcp_add_observations(
                &conn,
                vec![rosemary::mcp::ObservationInput {
                    entity_name: name,
                    contents,
                }],
            )
            .await?;
            info!("Observation added.");
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
            let graph = rosemary::db::mcp_read_graph(&conn).await?;
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
