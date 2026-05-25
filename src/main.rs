use anyhow::Result;
use clap::{Parser, Subcommand};
#[cfg(feature = "documents")]
use rosemary::paths::RosemaryPaths;
#[cfg(feature = "documents")]
use std::path::Path;
#[cfg(feature = "documents")]
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "rosemary")]
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
    AddObservations { name: String, content: String },
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

#[cfg(feature = "documents")]
async fn init_vector(
    paths: &RosemaryPaths,
) -> Result<(
    rosemary::vector::VectorStore,
    Arc<dyn rosemary::embed::EmbeddingProvider>,
)> {
    let lance_path = std::env::var("LANCEDB_PATH")
        .unwrap_or_else(|_| paths.data_dir.join("lancedb").to_str().unwrap().to_string());
    let store = rosemary::vector::VectorStore::new(&lance_path).await?;
    let embedder: Arc<dyn rosemary::embed::EmbeddingProvider> =
        if std::env::var("ROSEMARY_EMBED_PROVIDER").as_deref() == Ok("claude") {
            anyhow::bail!("ClaudeProvider not yet implemented")
        } else {
            let cache_dir = std::env::var("FASTEMBED_CACHE_DIR")
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

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
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
    let (_db, conn) = rosemary::db::init_db().await?;

    // Vector store + embedder are only initialised for commands that need them.
    // Graph-only operations (create-entities, read-graph, etc.) skip the heavy
    // fastembed model load entirely.
    if needs_vector(&cli.command) {
        #[cfg(feature = "documents")]
        {
            let (store, embedder) = init_vector(&paths).await?;
            match cli.command {
                Commands::Ingest { path } => {
                    let p = Path::new(&path);
                    if p.is_dir() {
                        println!("Ingesting directory: {:?}...", p);
                        let count =
                            rosemary::ingest::ingest_dir(p, &conn, &store, embedder.as_ref())
                                .await?;
                        println!("Done. Ingested {} files.", count);
                    } else {
                        println!("Ingesting file: {:?}...", p);
                        rosemary::ingest::ingest_file(p, &conn, &store, embedder.as_ref()).await?;
                        println!("Done.");
                    }
                }
                Commands::Query { query } => {
                    println!("Searching: {}...", query);
                    let results =
                        rosemary::recall::recall(&query, &conn, &store, embedder.as_ref(), 5)
                            .await?;
                    if results.is_empty() {
                        println!("No results found.");
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
                    let topics_root = std::env::var("ROSEMARY_TOPICS_DIR")
                        .unwrap_or_else(|_| paths.topics_dir.to_str().unwrap().to_string());
                    let pruned = rosemary::compact::prune_old_sessions(&topics_root, older_than)?;
                    println!("Pruned {} old session files.", pruned);

                    let clusters =
                        rosemary::compact::find_duplicate_clusters(&store, &conn, 0.85).await?;
                    println!("Found {} near-duplicate topic clusters.", clusters.len());

                    println!("Syncing Graph to Markdown...");
                    let synced =
                        rosemary::compact::sync_graph_to_markdown(&conn, &store, embedder.as_ref())
                            .await?;
                    println!("Done. Synced {} entities to Markdown.", synced);
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
            println!("Entity '{}' created.", name);
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
            println!("Relation created.");
        }
        Commands::AddObservations { name, content } => {
            rosemary::db::mcp_add_observations(
                &conn,
                vec![rosemary::mcp::ObservationInput {
                    entity_name: name,
                    contents: vec![content],
                }],
            )
            .await?;
            println!("Observation added.");
        }
        Commands::DeleteEntities { names } => {
            rosemary::db::mcp_delete_entities(&conn, names).await?;
            println!("Entities deleted.");
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
            println!("Observations deleted.");
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
            println!("Relations deleted.");
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
