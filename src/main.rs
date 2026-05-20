use anyhow::Result;
use clap::{Parser, Subcommand};
use rosemary::paths::RosemaryPaths;
use std::path::Path;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "rosemary")]
#[command(about = "Rosemary: Knowledge Base & Memory CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest a file or directory into the knowledge base
    Ingest {
        /// Path to file or directory
        path: String,
    },
    /// Query topics or chunks using hybrid semantic + keyword search
    Query {
        /// Query string
        query: String,
    },
    /// Create new entities in the knowledge graph
    CreateEntities { entities: String },
    /// Create relations between entities
    CreateRelations { relations: String },
    /// Add observations to existing entities
    AddObservations { observations: String },
    /// Delete entities and their relations
    DeleteEntities { names: String },
    /// Delete specific observations
    DeleteObservations { deletions: String },
    /// Delete specific relations
    DeleteRelations { relations: String },
    /// Read the entire knowledge graph
    ReadGraph,
    /// Search for nodes
    SearchNodes { query: String },
    /// Retrieve specific nodes by name
    OpenNodes { names: String },
    /// Merge near-duplicate topics, prune sessions, and sync Graph to MD
    Compact {
        /// Prune sessions older than N days
        #[arg(long, default_value = "90")]
        older_than: u32,
    },
    /// Start the MCP stdio server (legacy/compatibility)
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    let paths = RosemaryPaths::resolve();

    let (_db, conn) = rosemary::db::init_db().await?;
    let lance_path = std::env::var("LANCEDB_PATH").unwrap_or_else(|_| {
        paths
            .data_dir
            .join("lancedb")
            .to_str()
            .unwrap()
            .to_string()
    });
    let store = rosemary::vector::VectorStore::new(&lance_path).await?;
    let embedder: Arc<dyn rosemary::embed::EmbeddingProvider> =
        match std::env::var("ROSEMARY_EMBED_PROVIDER").as_deref() {
            Ok("claude") => anyhow::bail!("ClaudeProvider not yet implemented"),
            _ => Arc::new(rosemary::embed::FastEmbedProvider::new()?),
        };

    if store.dim() != embedder.dim() {
        anyhow::bail!(
            "Vector store dimension mismatch: store={}, embedder={}",
            store.dim(),
            embedder.dim()
        );
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Ingest { path } => {
            let p = Path::new(&path);
            if p.is_dir() {
                println!("Ingesting directory: {:?}...", p);
                let count = rosemary::ingest::ingest_dir(p, &conn, &store, embedder.as_ref()).await?;
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
                rosemary::recall::recall(&query, &conn, &store, embedder.as_ref(), 5).await?;
            if results.is_empty() {
                println!("No results found.");
            } else {
                for r in results {
                    // Concise format: Title (Score) | Path
                    println!("{:<20} | (score: {:.2}) | {}", r.title, r.score, r.file_path);
                }
            }
        }
        Commands::CreateEntities { entities } => {
            let params: rosemary::mcp::CreateEntitiesParams = serde_json::from_str(&entities)?;
            rosemary::db::mcp_create_entities(&conn, params.entities).await?;
            println!("Entities created.");
        }
        Commands::CreateRelations { relations } => {
            let params: rosemary::mcp::CreateRelationsParams = serde_json::from_str(&relations)?;
            rosemary::db::mcp_create_relations(&conn, params.relations).await?;
            println!("Relations created.");
        }
        Commands::AddObservations { observations } => {
            let params: rosemary::mcp::AddObservationsParams = serde_json::from_str(&observations)?;
            rosemary::db::mcp_add_observations(&conn, params.observations).await?;
            println!("Observations added.");
        }
        Commands::DeleteEntities { names } => {
            let names: Vec<String> = serde_json::from_str(&names)?;
            rosemary::db::mcp_delete_entities(&conn, names).await?;
            println!("Entities deleted.");
        }
        Commands::DeleteObservations { deletions } => {
            let params: rosemary::mcp::DeleteObservationsParams = serde_json::from_str(&deletions)?;
            rosemary::db::mcp_delete_observations(&conn, params.deletions).await?;
            println!("Observations deleted.");
        }
        Commands::DeleteRelations { relations } => {
            let params: rosemary::mcp::DeleteRelationsParams = serde_json::from_str(&relations)?;
            rosemary::db::mcp_delete_relations(&conn, params.relations).await?;
            println!("Relations deleted.");
        }
        Commands::ReadGraph => {
            let graph = rosemary::db::mcp_read_graph(&conn).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::SearchNodes { query } => {
            let graph = rosemary::db::mcp_search_nodes(&conn, &query).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::OpenNodes { names } => {
            let names: Vec<String> = serde_json::from_str(&names)?;
            let graph = rosemary::db::mcp_open_nodes(&conn, names).await?;
            println!("{}", serde_json::to_string_pretty(&graph)?);
        }
        Commands::Compact { older_than } => {
            let kb_root = std::env::var("KB_ROOT")
                .unwrap_or_else(|_| paths.kb_dir.to_str().unwrap().to_string());
            let pruned = rosemary::compact::prune_old_sessions(&kb_root, older_than)?;
            println!("Pruned {} old session files.", pruned);

            let clusters = rosemary::compact::find_duplicate_clusters(&store, &conn, 0.85).await?;
            println!("Found {} near-duplicate topic clusters.", clusters.len());

            println!("Syncing Graph to Markdown...");
            let synced = rosemary::compact::sync_graph_to_markdown(&conn, &store, embedder.as_ref()).await?;
            println!("Done. Synced {} entities to Markdown.", synced);
        }
        Commands::Mcp => {
            rosemary::mcp::run_server(conn).await?;
        }
    }

    Ok(())
}
