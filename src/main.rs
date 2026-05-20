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
    /// Add a new entity to the knowledge graph
    AddEntity {
        name: String,
        #[arg(rename_all = "snake_case")]
        entity_type: String,
    },
    /// Add an observation to an existing entity
    AddObservation {
        name: String,
        content: String,
    },
    /// Relate two entities
    Relate {
        from: String,
        to: String,
        relation: String,
    },
    /// List all entities and relations (the whole graph)
    List,
    /// Remove an entity and its associated relations/observations
    DeleteEntity {
        name: String,
    },
    /// Remove specific observations from an entity
    DeleteObservation {
        name: String,
        /// Optional: specific content to delete. If omitted, deletes all obs for entity (careful!)
        content: Option<String>,
    },
    /// Open a specific entity to see its details
    Open {
        name: String,
    },
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
        Commands::AddEntity { name, entity_type } => {
            rosemary::db::mcp_create_entities(&conn, vec![rosemary::mcp::EntityInput {
                name: name.clone(),
                entity_type,
                observations: vec![],
            }]).await?;
            println!("Entity '{}' added.", name);
        }
        Commands::AddObservation { name, content } => {
            rosemary::db::mcp_add_observations(&conn, vec![rosemary::mcp::ObservationInput {
                entity_name: name.clone(),
                contents: vec![content],
            }]).await?;
            println!("Observation added to '{}'.", name);
        }
        Commands::Relate { from, to, relation } => {
            rosemary::db::mcp_create_relations(&conn, vec![rosemary::mcp::RelationInput {
                from: from.clone(),
                to: to.clone(),
                relation_type: relation,
            }]).await?;
            println!("Relation {} -> {} added.", from, to);
        }
        Commands::List => {
            let graph = rosemary::db::mcp_read_graph(&conn).await?;
            println!("Entities:");
            for e in graph.entities {
                println!("- {} ({})", e.name, e.entity_type);
                for o in e.observations {
                    println!("  * {}", o);
                }
            }
            println!("\nRelations:");
            for r in graph.relations {
                println!("- {} --({})--> {}", r.from, r.relation_type, r.to);
            }
        }
        Commands::Open { name } => {
            let graph = rosemary::db::mcp_open_nodes(&conn, vec![name.clone()]).await?;
            for e in graph.entities {
                println!("Entity: {} ({})", e.name, e.entity_type);
                println!("Observations:");
                for o in e.observations {
                    println!("  * {}", o);
                }
            }
            println!("Relations:");
            for r in graph.relations {
                println!("- {} --({})--> {}", r.from, r.relation_type, r.to);
            }
        }
        Commands::DeleteEntity { name } => {
            rosemary::db::mcp_delete_entities(&conn, vec![name.clone()]).await?;
            println!("Entity '{}' deleted.", name);
        }
        Commands::DeleteObservation { name, content } => {
            if let Some(c) = content {
                rosemary::db::mcp_delete_observations(&conn, vec![rosemary::mcp::ObservationDeletion {
                    entity_name: name.clone(),
                    observations: vec![c],
                }]).await?;
                println!("Observation deleted from '{}'.", name);
            } else {
                // If no content, we need a way to clear all observations. 
                // For now, let's just delete the entity and re-add it if that's the intent, 
                // or we could add a dedicated 'clear-obs' later.
                println!("Warning: No content specified. Use delete-entity to remove everything.");
            }
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
