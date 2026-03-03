use anyhow::Result;
use libsql::{Builder, Connection, Database};
use std::env;

pub async fn init_db() -> Result<(Database, Connection)> {
    let db_path = env::var("DATABASE_URL").unwrap_or_else(|_| "rosemary.db".to_string());
    let db = Builder::new_local(&db_path).build().await?;
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
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            entity_type TEXT
        )",
        (),
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS relations (
            from_id TEXT,
            to_id TEXT,
            relation_type TEXT,
            PRIMARY KEY (from_id, to_id, relation_type)
        )",
        (),
    )
    .await?;

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
    )
    .await?;

    // create vector index for semantic search
    conn.execute(
        "CREATE INDEX IF NOT EXISTS gists_idx ON gists (libsql_vector_idx(embedding, 'metric=cosine'))",
        (),
    ).await?;

    Ok((db, conn))
}

pub async fn search_topics(
    conn: &Connection,
    query: &str,
) -> Result<Vec<(String, String, String)>> {
    let sql = "SELECT id, title, file_path FROM topics WHERE title LIKE ?1 OR id IN (SELECT topic_id FROM gists WHERE content LIKE ?1)";
    let pattern = format!("%{}%", query);
    let mut rows = conn.query(sql, libsql::params![pattern]).await?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let title: String = row.get(1)?;
        let path: String = row.get(2)?;
        results.push((id, title, path));
    }
    Ok(results)
}

pub async fn get_related_topics(
    conn: &Connection,
    topic_id: &str,
) -> Result<Vec<(String, String)>> {
    let sql = "SELECT to_id, relation_type FROM relations WHERE from_id = ?1
               UNION
               SELECT from_id, relation_type FROM relations WHERE to_id = ?1";
    let mut rows = conn.query(sql, libsql::params![topic_id]).await?;

    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let relation: String = row.get(1)?;
        results.push((id, relation));
    }
    Ok(results)
}
