use anyhow::Result;
use libsql::{Builder, Connection, Database};
use std::env;

pub async fn init_db() -> Result<(Database, Connection)> {
    let db_path = env::var("DATABASE_URL").unwrap_or_else(|_| "rosemary.db".to_string());
    let db = Builder::new_local(&db_path).build().await?;
    let conn = db.connect()?;

    conn.execute("PRAGMA foreign_keys = ON", ()).await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS topics (
            id        TEXT PRIMARY KEY,
            title     TEXT NOT NULL,
            file_path TEXT NOT NULL,
            body      TEXT NOT NULL DEFAULT '',
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    )
    .await?;

    // FTS5 for full-text keyword search
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS topics_fts
         USING fts5(title, body, content='topics', content_rowid='rowid', tokenize='porter unicode61')",
        (),
    ).await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id        TEXT PRIMARY KEY,
            summary   TEXT NOT NULL,
            file_path TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS entities (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL UNIQUE,
            entity_type TEXT
        )",
        (),
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS relations (
            from_id       TEXT NOT NULL,
            to_id         TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            PRIMARY KEY (from_id, to_id, relation_type),
            FOREIGN KEY (from_id) REFERENCES entities(id),
            FOREIGN KEY (to_id)   REFERENCES entities(id)
        )",
        (),
    )
    .await?;

    // Triggers to keep FTS5 in sync with topics
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS topics_ai AFTER INSERT ON topics BEGIN
            INSERT INTO topics_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
        END",
        (),
    )
    .await?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS topics_ad AFTER DELETE ON topics BEGIN
            INSERT INTO topics_fts(topics_fts, rowid, title, body) VALUES('delete', old.rowid, old.title, old.body);
        END",
        (),
    ).await?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS topics_au AFTER UPDATE ON topics BEGIN
            INSERT INTO topics_fts(topics_fts, rowid, title, body) VALUES('delete', old.rowid, old.title, old.body);
            INSERT INTO topics_fts(rowid, title, body) VALUES (new.rowid, new.title, new.body);
        END",
        (),
    ).await?;

    Ok((db, conn))
}

/// FTS5 keyword search — returns (id, title, file_path, bm25_score)
pub async fn search_fts(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<(String, String, String, f64)>> {
    let sql = "SELECT t.id, t.title, t.file_path, bm25(topics_fts) AS score
               FROM topics_fts
               JOIN topics t ON topics_fts.rowid = t.rowid
               WHERE topics_fts MATCH ?1
               ORDER BY score
               LIMIT ?2";
    let mut rows = conn
        .query(sql, libsql::params![query, limit as i64])
        .await?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push((
            row.get::<String>(0)?,
            row.get::<String>(1)?,
            row.get::<String>(2)?,
            row.get::<f64>(3)?,
        ));
    }
    Ok(results)
}

pub async fn upsert_topic(
    conn: &Connection,
    id: &str,
    title: &str,
    file_path: &str,
    body: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO topics (id, title, file_path, body) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET 
            title=excluded.title, 
            file_path=excluded.file_path,
            body=excluded.body,
            updated_at=CURRENT_TIMESTAMP",
        libsql::params![id, title, file_path, body],
    )
    .await?;
    Ok(())
}

pub async fn upsert_entity(
    conn: &Connection,
    id: &str,
    name: &str,
    entity_type: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO entities (id, name, entity_type) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, entity_type=excluded.entity_type",
        libsql::params![id, name, entity_type],
    )
    .await?;
    Ok(())
}

pub async fn insert_relation(
    conn: &Connection,
    from_id: &str,
    to_id: &str,
    relation_type: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO relations (from_id, to_id, relation_type) VALUES (?1, ?2, ?3)",
        libsql::params![from_id, to_id, relation_type],
    )
    .await?;
    Ok(())
}

pub async fn get_related(
    conn: &Connection,
    entity_id: &str,
) -> Result<Vec<(String, String, String)>> {
    let sql = "SELECT e.name, r.to_id, r.relation_type FROM relations r
               JOIN entities e ON e.id = r.to_id
               WHERE r.from_id = ?1
               UNION
               SELECT e.name, r.from_id, r.relation_type FROM relations r
               JOIN entities e ON e.id = r.from_id
               WHERE r.to_id = ?1";
    let mut rows = conn.query(sql, libsql::params![entity_id]).await?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push((row.get(0)?, row.get(1)?, row.get(2)?));
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_init_creates_all_tables() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        // FTS5 table should exist
        let mut rows = conn
            .query("SELECT name FROM sqlite_master WHERE name='topics_fts'", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap();
        assert!(row.is_some(), "topics_fts table missing");
    }

    #[tokio::test]
    async fn test_fts_search_finds_topic() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        conn.execute(
            "INSERT INTO topics (id, title, file_path) VALUES ('rust-pin', 'Rust Pinning', 'kb/topics/rust-pinning.md')",
            (),
        ).await.unwrap();
        conn.execute(
            "INSERT INTO topics_fts (rowid, title, body) VALUES ((SELECT rowid FROM topics WHERE id='rust-pin'), 'Rust Pinning', 'pinning is a mechanism...')",
            (),
        ).await.unwrap();

        let results = search_fts(&conn, "pinning", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "Rust Pinning");
    }
}
