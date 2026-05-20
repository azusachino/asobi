use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::Path;
use std::sync::Arc;
use rosemary::paths::RosemaryPaths;

#[derive(Parser)]
#[command(name = "rosemary")]
#[command(about = "Rosemary: Knowledge Base & MCP Memory Server", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Knowledge Base operations
    Kb {
        #[command(subcommand)]
        action: KbAction,
    },
    /// Model Context Protocol (MCP) server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum KbAction {
    /// Ingest a file or directory into the knowledge base
    Ingest {
        /// Path to file or directory
        path: String,
    },
    /// Compact session transcript into topic files + session summary
    Digest {
        /// Path to transcript file; reads from stdin if omitted
        #[arg(short, long)]
        file: Option<String>,
    },
    /// Recall topics or chunks using hybrid semantic + keyword search
    Recall {
        /// Query string
        query: String,
    },
    /// Relate two entities
    Relate {
        from: String,
        to: String,
        relation: String,
    },
    /// Merge near-duplicate topics, prune sessions, and sync Graph to MD
    Compact {
        /// Prune sessions older than N days
        #[arg(long, default_value = "90")]
        older_than: u32,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// Start the MCP stdio server
    Start,
}

#[tokio::main]
async fn main() -> Result<()> {
    // load environment variables
    let _ = dotenvy::dotenv();
    let paths = RosemaryPaths::resolve();

    // initialize unified libSQL database
    let (_db, conn) = rosemary::db::init_db().await?;

    // initialize vector store
    let lance_path = std::env::var("LANCEDB_PATH")
        .unwrap_or_else(|_| paths.data_dir.join("lancedb").to_str().unwrap().to_string());
    let store = rosemary::vector::VectorStore::new(&lance_path).await?;

    // initialize embedding provider
    let embedder: Arc<dyn rosemary::embed::EmbeddingProvider> =
        match std::env::var("ROSEMARY_EMBED_PROVIDER").as_deref() {
            Ok("claude") => anyhow::bail!("ClaudeProvider not yet implemented"),
            _ => Arc::new(rosemary::embed::FastEmbedProvider::new()?),
        };

    // Assert dimension match
    if store.dim() != embedder.dim() {
        anyhow::bail!(
            "Vector store dimension mismatch: store={}, embedder={}",
            store.dim(),
            embedder.dim()
        );
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Kb { action } => match action {
            KbAction::Ingest { path } => {
                let p = Path::new(&path);
                if p.is_dir() {
                    println!("Ingesting directory: {:?}...", p);
                    let count =
                        rosemary::ingest::ingest_dir(p, &conn, &store, embedder.as_ref()).await?;
                    println!("Done. Ingested {} files.", count);
                } else {
                    println!("Ingesting file: {:?}...", p);
                    rosemary::ingest::ingest_file(p, &conn, &store, embedder.as_ref()).await?;
                    println!("Done.");
                }
            }
            KbAction::Digest { file } => {
                let transcript = match file {
                    Some(p) => std::fs::read_to_string(&p)?,
                    None => {
                        use std::io::Read;
                        let mut buf = String::new();
                        std::io::stdin().read_to_string(&mut buf)?;
                        buf
                    }
                };

                println!("Digesting session...");
                let output = rosemary::digest::call_digest_llm(&transcript).await?;

                let kb_root = std::env::var("KB_ROOT")
                    .unwrap_or_else(|_| paths.kb_dir.to_str().unwrap().to_string());

                for topic in &output.topics {
                    println!("  topic: {}", topic.title);
                    let path = rosemary::kb::save_markdown(&topic.title, &topic.content)?;
                    rosemary::ingest::ingest_file(&path, &conn, &store, embedder.as_ref()).await?;
                }

                let session_path =
                    rosemary::digest::write_session_file(&kb_root, &output.session_summary)?;

                // Insert into sessions table
                let session_id = session_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                conn.execute(
                    "INSERT INTO sessions (id, summary, file_path) VALUES (?1, ?2, ?3)",
                    libsql::params![
                        session_id,
                        output.session_summary,
                        session_path.to_str().unwrap()
                    ],
                )
                .await?;

                // Ingest session summary as a chunk
                rosemary::ingest::ingest_file(&session_path, &conn, &store, embedder.as_ref()).await?;

                println!("Session summary: {:?}", session_path);
                println!("Done. {} topics ingested.", output.topics.len());
            }
            KbAction::Recall { query } => {
                println!("Searching: {}...", query);
                let results =
                    rosemary::recall::recall(&query, &conn, &store, embedder.as_ref(), 5).await?;
                if results.is_empty() {
                    println!("No results found.");
                } else {
                    for r in results {
                        println!("\n# {} (score: {:.2})", r.title, r.score);
                        println!("Path: {}", r.file_path);
                        if !r.snippet.is_empty() {
                            println!(
                                "Snippet: {}...",
                                &r.snippet.chars().take(120).collect::<String>()
                            );
                        }
                    }
                }
            }
            KbAction::Relate { from, to, relation } => {
                println!("Relating {} --({})--> {}...", from, relation, to);

                // Ensure both entities exist before inserting the relation
                let from_id = slug::slugify(&from);
                let to_id = slug::slugify(&to);

                rosemary::db::upsert_entity(&conn, &from_id, &from, "concept").await?;
                rosemary::db::upsert_entity(&conn, &to_id, &to, "concept").await?;
                rosemary::db::insert_relation(&conn, &from_id, &to_id, &relation).await?;

                println!("Done.");
            }
            KbAction::Compact { older_than } => {
                let kb_root = std::env::var("KB_ROOT")
                    .unwrap_or_else(|_| paths.kb_dir.to_str().unwrap().to_string());
                
                let pruned = rosemary::compact::prune_old_sessions(&kb_root, older_than)?;
                println!("Pruned {} old session files.", pruned);

                let clusters = rosemary::compact::find_duplicate_clusters(&store, &conn, 0.85).await?;
                println!("Found {} near-duplicate topic clusters.", clusters.len());
                for cluster in &clusters {
                    println!("  Cluster: {:?}", cluster);
                }

                println!("Syncing Graph to Markdown...");
                let synced = rosemary::compact::sync_graph_to_markdown(&conn, &store, embedder.as_ref()).await?;
                println!("Done. Synced {} entities to Markdown.", synced);
            }
        },
        Commands::Mcp { action } => match action {
            McpAction::Start => {
                // MCP server runs until process is killed
                rosemary::mcp::run_server(conn).await?;
            }
        },
    }

    Ok(())
}
