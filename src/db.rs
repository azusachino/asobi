use anyhow::Result;
use libsql::{Builder, Connection, Database};
use std::env;

pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::RosemaryPaths::resolve();
    if !paths.data_dir.exists() {
        std::fs::create_dir_all(&paths.data_dir)?;
    }

    let db_path =
        env::var("DATABASE_URL").unwrap_or_else(|_| paths.db_path().to_str().unwrap().to_string());
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

    // Graph Tier (Hot)
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_entities (
            name        TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_observations (
            id          TEXT PRIMARY KEY,
            entity_name TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
        )",
        (),
    )
    .await?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_relations (
            from_entity   TEXT NOT NULL,
            to_entity     TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (from_entity, to_entity, relation_type),
            FOREIGN KEY (from_entity) REFERENCES mcp_entities(name) ON DELETE CASCADE,
            FOREIGN KEY (to_entity)   REFERENCES mcp_entities(name) ON DELETE CASCADE
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

    // FTS5 for graph observation search (porter stemming, BM25 ranking)
    conn.execute(
        "CREATE VIRTUAL TABLE IF NOT EXISTS mcp_obs_fts
         USING fts5(content, content='mcp_observations', content_rowid='rowid', tokenize='porter unicode61')",
        (),
    ).await?;

    // Triggers to keep mcp_obs_fts in sync with mcp_observations
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS mcp_obs_ai AFTER INSERT ON mcp_observations BEGIN
            INSERT INTO mcp_obs_fts(rowid, content) VALUES (new.rowid, new.content);
        END",
        (),
    )
    .await?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS mcp_obs_ad AFTER DELETE ON mcp_observations BEGIN
            INSERT INTO mcp_obs_fts(mcp_obs_fts, rowid, content) VALUES('delete', old.rowid, old.content);
        END",
        (),
    ).await?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS mcp_obs_au AFTER UPDATE ON mcp_observations BEGIN
            INSERT INTO mcp_obs_fts(mcp_obs_fts, rowid, content) VALUES('delete', old.rowid, old.content);
            INSERT INTO mcp_obs_fts(rowid, content) VALUES (new.rowid, new.content);
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

pub async fn mcp_create_entities(
    conn: &Connection,
    entities: Vec<crate::mcp::EntityInput>,
) -> Result<()> {
    for ent in entities {
        conn.execute(
            "INSERT OR IGNORE INTO mcp_entities (name, entity_type) VALUES (?1, ?2)",
            libsql::params![ent.name.clone(), ent.entity_type],
        )
        .await?;
        for obs in ent.observations {
            conn.execute(
                "INSERT INTO mcp_observations (id, entity_name, content) VALUES (?1, ?2, ?3)",
                libsql::params![uuid::Uuid::new_v4().to_string(), ent.name.clone(), obs],
            )
            .await?;
        }
    }
    Ok(())
}

pub async fn mcp_add_observations(
    conn: &Connection,
    observations: Vec<crate::mcp::ObservationInput>,
) -> Result<()> {
    for obs_batch in observations {
        for content in obs_batch.contents {
            conn.execute(
                "INSERT INTO mcp_observations (id, entity_name, content) VALUES (?1, ?2, ?3)",
                libsql::params![
                    uuid::Uuid::new_v4().to_string(),
                    obs_batch.entity_name.clone(),
                    content
                ],
            )
            .await?;
        }
    }
    Ok(())
}

pub async fn mcp_create_relations(
    conn: &Connection,
    relations: Vec<crate::mcp::RelationInput>,
) -> Result<()> {
    for rel in relations {
        conn.execute(
            "INSERT OR REPLACE INTO mcp_relations (from_entity, to_entity, relation_type) VALUES (?1, ?2, ?3)",
            libsql::params![rel.from, rel.to, rel.relation_type],
        ).await?;
    }
    Ok(())
}

pub async fn mcp_delete_entities(conn: &Connection, names: Vec<String>) -> Result<()> {
    for name in names {
        conn.execute(
            "DELETE FROM mcp_entities WHERE name = ?1",
            libsql::params![name],
        )
        .await?;
    }
    Ok(())
}

pub async fn mcp_delete_observations(
    conn: &Connection,
    deletions: Vec<crate::mcp::ObservationDeletion>,
) -> Result<()> {
    for del in deletions {
        for obs in del.observations {
            conn.execute(
                "DELETE FROM mcp_observations WHERE entity_name = ?1 AND content = ?2",
                libsql::params![del.entity_name.clone(), obs],
            )
            .await?;
        }
    }
    Ok(())
}

pub async fn mcp_delete_relations(
    conn: &Connection,
    relations: Vec<crate::mcp::RelationInput>,
) -> Result<()> {
    for rel in relations {
        conn.execute(
            "DELETE FROM mcp_relations WHERE from_entity = ?1 AND to_entity = ?2 AND relation_type = ?3",
            libsql::params![rel.from, rel.to, rel.relation_type],
        ).await?;
    }
    Ok(())
}

pub async fn mcp_read_graph(conn: &Connection) -> Result<crate::mcp::Graph> {
    let mut entities = Vec::new();
    let mut rows = conn
        .query("SELECT name, entity_type FROM mcp_entities", ())
        .await?;
    while let Some(row) = rows.next().await? {
        let name: String = row.get(0)?;
        let entity_type: String = row.get(1)?;

        let mut obs_rows = conn
            .query(
                "SELECT content FROM mcp_observations WHERE entity_name = ?1",
                libsql::params![name.clone()],
            )
            .await?;
        let mut observations = Vec::new();
        while let Some(obs_row) = obs_rows.next().await? {
            observations.push(obs_row.get(0)?);
        }

        entities.push(crate::mcp::EntityOutput {
            name,
            entity_type,
            observations,
        });
    }

    let mut relations = Vec::new();
    let mut rel_rows = conn
        .query(
            "SELECT from_entity, to_entity, relation_type FROM mcp_relations",
            (),
        )
        .await?;
    while let Some(row) = rel_rows.next().await? {
        relations.push(crate::mcp::RelationInput {
            from: row.get(0)?,
            to: row.get(1)?,
            relation_type: row.get(2)?,
        });
    }

    Ok(crate::mcp::Graph {
        entities,
        relations,
    })
}

pub async fn mcp_search_nodes(conn: &Connection, query: &str) -> Result<crate::mcp::Graph> {
    let mut entity_names: Vec<String> = Vec::new();

    // Primary: FTS5 on observation content — porter stemming + BM25 ranking.
    // Wrapped in an async block so any error (invalid syntax, bad token) is
    // caught at the boundary and we fall through to the LIKE path.
    let fts_sql = "SELECT DISTINCT o.entity_name
                   FROM mcp_obs_fts f
                   JOIN mcp_observations o ON f.rowid = o.rowid
                   WHERE mcp_obs_fts MATCH ?1
                   ORDER BY bm25(mcp_obs_fts)";
    let fts_hits: Vec<String> = async {
        let mut rows = conn.query(fts_sql, libsql::params![query]).await?;
        let mut names = Vec::new();
        while let Some(row) = rows.next().await? {
            names.push(row.get::<String>(0)?);
        }
        Ok::<Vec<String>, anyhow::Error>(names)
    }
    .await
    .unwrap_or_default();
    for name in fts_hits {
        if !entity_names.contains(&name) {
            entity_names.push(name);
        }
    }

    // Secondary: LIKE on entity name / type — always runs, catches exact-name
    // lookups and entity types that aren't in observations.
    let pattern = format!("%{}%", query);
    let mut rows = conn
        .query(
            "SELECT name FROM mcp_entities WHERE name LIKE ?1 OR entity_type LIKE ?1",
            libsql::params![pattern],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let name: String = row.get(0)?;
        if !entity_names.contains(&name) {
            entity_names.push(name);
        }
    }

    // Load full entity data for each matched name (preserves FTS rank order).
    let mut entities = Vec::new();
    for name in &entity_names {
        let mut rows = conn
            .query(
                "SELECT entity_type FROM mcp_entities WHERE name = ?1",
                libsql::params![name.clone()],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let entity_type: String = row.get(0)?;
            let mut obs_rows = conn
                .query(
                    "SELECT content FROM mcp_observations WHERE entity_name = ?1",
                    libsql::params![name.clone()],
                )
                .await?;
            let mut observations = Vec::new();
            while let Some(obs_row) = obs_rows.next().await? {
                observations.push(obs_row.get(0)?);
            }
            entities.push(crate::mcp::EntityOutput {
                name: name.clone(),
                entity_type,
                observations,
            });
        }
    }

    // Relations between matched entities.
    let mut relations = Vec::new();
    if !entity_names.is_empty() {
        let placeholders = entity_names
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT from_entity, to_entity, relation_type FROM mcp_relations
             WHERE from_entity IN ({}) AND to_entity IN ({})",
            placeholders, placeholders
        );
        let mut params = Vec::new();
        for name in &entity_names {
            params.push(libsql::Value::from(name.clone()));
        }
        for name in &entity_names {
            params.push(libsql::Value::from(name.clone()));
        }
        let mut rel_rows = conn.query(&sql, params).await?;
        while let Some(row) = rel_rows.next().await? {
            relations.push(crate::mcp::RelationInput {
                from: row.get(0)?,
                to: row.get(1)?,
                relation_type: row.get(2)?,
            });
        }
    }

    Ok(crate::mcp::Graph {
        entities,
        relations,
    })
}

pub async fn mcp_open_nodes(conn: &Connection, names: Vec<String>) -> Result<crate::mcp::Graph> {
    let mut entities = Vec::new();
    for name in &names {
        let mut rows = conn
            .query(
                "SELECT entity_type FROM mcp_entities WHERE name = ?1",
                libsql::params![name.clone()],
            )
            .await?;
        if let Some(row) = rows.next().await? {
            let entity_type: String = row.get(0)?;

            let mut obs_rows = conn
                .query(
                    "SELECT content FROM mcp_observations WHERE entity_name = ?1",
                    libsql::params![name.clone()],
                )
                .await?;
            let mut observations = Vec::new();
            while let Some(obs_row) = obs_rows.next().await? {
                observations.push(obs_row.get(0)?);
            }

            entities.push(crate::mcp::EntityOutput {
                name: name.clone(),
                entity_type,
                observations,
            });
        }
    }

    let mut relations = Vec::new();
    if !names.is_empty() {
        let placeholders = names.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT from_entity, to_entity, relation_type FROM mcp_relations 
             WHERE from_entity IN ({}) AND to_entity IN ({})",
            placeholders, placeholders
        );

        let mut params = Vec::new();
        for name in &names {
            params.push(libsql::Value::from(name.clone()));
        }
        for name in &names {
            params.push(libsql::Value::from(name.clone()));
        }

        let mut rel_rows = conn.query(&sql, params).await?;
        while let Some(row) = rel_rows.next().await? {
            relations.push(crate::mcp::RelationInput {
                from: row.get(0)?,
                to: row.get(1)?,
                relation_type: row.get(2)?,
            });
        }
    }

    Ok(crate::mcp::Graph {
        entities,
        relations,
    })
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
            "INSERT INTO topics (id, title, file_path) VALUES ('rust-pin', 'Rust Pinning', '.rosemary/topics/rust-pinning.md')",
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

    async fn seed_entity(conn: &Connection, name: &str, entity_type: &str, obs: &[&str]) {
        mcp_create_entities(
            conn,
            vec![crate::mcp::EntityInput {
                name: name.to_string(),
                entity_type: entity_type.to_string(),
                observations: obs.iter().map(|s| s.to_string()).collect(),
            }],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_search_nodes_stemming() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("DATABASE_URL", dir.path().join("test.db").to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(
            &conn,
            "async-patterns",
            "concept",
            &["running async tasks efficiently"],
        )
        .await;

        // "run" should match "running" via porter stemming
        let graph = mcp_search_nodes(&conn, "run").await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "async-patterns");
    }

    #[tokio::test]
    async fn test_search_nodes_entity_name_fallback() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("DATABASE_URL", dir.path().join("test.db").to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        // Entity with no observations — FTS finds nothing, LIKE fallback finds by name
        seed_entity(&conn, "UserPreferences", "preference", &[]).await;

        let graph = mcp_search_nodes(&conn, "UserPreferences").await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "UserPreferences");
    }

    #[tokio::test]
    async fn test_search_nodes_bm25_ordering() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("DATABASE_URL", dir.path().join("test.db").to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        // "alpha" has both query words; "beta" has only one — alpha should rank first
        seed_entity(&conn, "alpha", "project", &["async tokio runtime patterns"]).await;
        seed_entity(&conn, "beta", "project", &["tokio scheduler"]).await;

        let graph = mcp_search_nodes(&conn, "async tokio").await.unwrap();
        assert!(!graph.entities.is_empty());
        assert_eq!(graph.entities[0].name, "alpha");
    }

    #[tokio::test]
    async fn test_search_nodes_invalid_fts_syntax_no_panic() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var("DATABASE_URL", dir.path().join("test.db").to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        // Invalid FTS5 syntax — must not panic, falls back to LIKE gracefully
        let result = mcp_search_nodes(&conn, "AND AND").await;
        assert!(result.is_ok());
    }
}
