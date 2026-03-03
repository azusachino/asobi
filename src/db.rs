use libsql::{Builder, Connection};
use anyhow::Result;

pub async fn init_db() -> Result<Connection> {
    let db = Builder::new_local("rosemary.db").build().await?;
    let conn = db.connect()?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS topics (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            slug TEXT NOT NULL,
            file_path TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    ).await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            entity_type TEXT
        )",
        (),
    ).await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS relations (
            from_id TEXT,
            to_id TEXT,
            relation_type TEXT,
            PRIMARY KEY (from_id, to_id, relation_type)
        )",
        (),
    ).await?;

    // gists table with vector support (384-dimensional embeddings are common for local models)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS gists (
            id TEXT PRIMARY KEY,
            topic_id TEXT,
            content TEXT NOT NULL,
            embedding F32_BLOB(384),
            FOREIGN KEY (topic_id) REFERENCES topics (id)
        )",
        (),
    ).await?;

    // create vector index for semantic search
    conn.execute(
        "CREATE INDEX IF NOT EXISTS gists_idx ON gists (libsql_vector_idx(embedding, 'metric=cosine'))",
        (),
    ).await?;

    Ok(conn)
}
