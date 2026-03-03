use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rosemary")]
#[command(about = "Personal Knowledge Base CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest a new topic and content
    Ingest { topic: String, content: String },
    /// Recall topics or gists using semantic search
    Recall { query: String },
    /// Relate two entities
    Relate {
        from: String,
        to: String,
        relation: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // load environment variables
    let _ = dotenvy::dotenv();

    // initialize unified libSQL database (topics, relations, and vectors)
    let (_db, conn) = rosemary::db::init_db().await?;

    let cli = Cli::parse();

    match cli.command {
        Commands::Ingest { topic, content } => {
            println!("Ingesting topic: {}...", topic);
            let path = rosemary::kb::save_markdown(&topic, &content)?;
            let slug = slug::slugify(&topic);

            // save to database
            conn.execute(
                "INSERT OR REPLACE INTO topics (id, title, slug, file_path) VALUES (?1, ?2, ?3, ?4)",
                libsql::params![slug.clone(), topic, slug, path.to_str().unwrap()],
            ).await?;

            println!("Saved metadata to database and markdown to: {:?}", path);
        }
        Commands::Recall { query } => {
            println!("Searching for: {}...", query);
            let results = rosemary::db::search_topics(&conn, &query).await?;
            if results.is_empty() {
                println!("No results found.");
            } else {
                for (id, title, path) in results {
                    println!("\n# {}", title);
                    println!("Path: {}", path);

                    let related = rosemary::db::get_related_topics(&conn, &id).await?;
                    if !related.is_empty() {
                        println!("Related:");
                        for (rel_id, rel_type) in related {
                            println!("  - [{}] -> {}", rel_type, rel_id);
                        }
                    }
                }
            }
        }
        Commands::Relate { from, to, relation } => {
            println!("Relating {} --({})--> {}...", from, relation, to);
            conn.execute(
                "INSERT OR REPLACE INTO relations (from_id, to_id, relation_type) VALUES (?1, ?2, ?3)",
                libsql::params![from, to, relation],
            ).await?;
        }
    }

    Ok(())
}
