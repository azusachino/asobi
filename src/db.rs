use anyhow::Result;
use libsql::{Builder, Connection, Database};
use std::collections::HashMap;
use std::env;

pub const DEFAULT_SEARCH_LIMIT: usize = 100;
pub use crate::constant::ENV_DATABASE_URL;

pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::RosemaryPaths::resolve();
    if !paths.data_dir.exists() {
        std::fs::create_dir_all(&paths.data_dir)?;
    }

    let db_path = env::var(ENV_DATABASE_URL)
        .unwrap_or_else(|_| paths.db_path().to_str().unwrap().to_string());
    let db = Builder::new_local(&db_path).build().await?;
    let conn = db.connect()?;

    conn.execute(crate::constant::PRAGMA_FOREIGN_KEYS_ON, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_TOPICS, ())
        .await?;

    // FTS5 for full-text keyword search
    conn.execute(crate::constant::SCHEMA_CREATE_TOPICS_FTS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_SESSIONS, ())
        .await?;

    // Graph Tier (Hot)
    conn.execute(crate::constant::SCHEMA_CREATE_MCP_ENTITIES, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_MCP_OBSERVATIONS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_IDX_MCP_OBSERVATIONS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_MCP_RELATIONS, ())
        .await?;

    // Document Tier (Vectors)
    conn.execute(crate::constant::SCHEMA_CREATE_CHUNKS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_IDX_CHUNKS_TOPIC_ID, ())
        .await?;

    // Vector index - metric=cosine is default
    conn.execute(crate::constant::SCHEMA_CREATE_IDX_CHUNKS_VECTOR, ())
        .await?;

    // Triggers to keep FTS5 in sync with topics
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AI, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AD, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AU, ())
        .await?;

    // FTS5 for graph observation search (porter stemming, BM25 ranking)
    conn.execute(crate::constant::SCHEMA_CREATE_MCP_OBS_FTS, ())
        .await?;

    // Triggers to keep mcp_obs_fts in sync with mcp_observations
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_MCP_OBS_AI, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_MCP_OBS_AD, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_MCP_OBS_AU, ())
        .await?;

    Ok((db, conn))
}

/// FTS5 keyword search — returns (id, title, file_path, bm25_score)
pub async fn search_fts(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<Vec<(String, String, String, f64)>> {
    let mut rows = conn
        .query(
            crate::constant::SQL_SEARCH_FTS,
            libsql::params![query, limit as i64],
        )
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
        crate::constant::SQL_UPSERT_TOPIC,
        libsql::params![id, title, file_path, body],
    )
    .await?;
    Ok(())
}

pub async fn mcp_create_entities(
    conn: &Connection,
    entities: Vec<crate::mcp::EntityInput>,
) -> Result<()> {
    let tx = conn.transaction().await?;
    for mut ent in entities {
        ent.name = crate::normalize::normalize_key(&ent.name);
        tx.execute(
            crate::constant::SQL_INSERT_ENTITY,
            libsql::params![ent.name.clone(), ent.entity_type],
        )
        .await?;
        for obs in ent.observations {
            tx.execute(
                crate::constant::SQL_INSERT_OBSERVATION,
                libsql::params![uuid::Uuid::new_v4().to_string(), ent.name.clone(), obs],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_add_observations(
    conn: &Connection,
    observations: Vec<crate::mcp::ObservationInput>,
) -> Result<()> {
    let tx = conn.transaction().await?;
    for mut obs_batch in observations {
        obs_batch.entity_name = crate::normalize::normalize_key(&obs_batch.entity_name);
        for content in obs_batch.contents {
            tx.execute(
                crate::constant::SQL_INSERT_OBSERVATION,
                libsql::params![
                    uuid::Uuid::new_v4().to_string(),
                    obs_batch.entity_name.clone(),
                    content
                ],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_create_relations(
    conn: &Connection,
    relations: Vec<crate::mcp::RelationInput>,
) -> Result<()> {
    let tx = conn.transaction().await?;
    for mut rel in relations {
        rel.from = crate::normalize::normalize_key(&rel.from);
        rel.to = crate::normalize::normalize_key(&rel.to);
        tx.execute(
            crate::constant::SQL_INSERT_RELATION,
            libsql::params![rel.from, rel.to, rel.relation_type],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_delete_entities(conn: &Connection, names: Vec<String>) -> Result<()> {
    let tx = conn.transaction().await?;
    for name in names {
        let norm_name = crate::normalize::normalize_key(&name);
        tx.execute(
            crate::constant::SQL_DELETE_ENTITY,
            libsql::params![norm_name],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_delete_observations(
    conn: &Connection,
    deletions: Vec<crate::mcp::ObservationDeletion>,
) -> Result<()> {
    let tx = conn.transaction().await?;
    for mut del in deletions {
        del.entity_name = crate::normalize::normalize_key(&del.entity_name);
        for obs in del.observations {
            tx.execute(
                crate::constant::SQL_DELETE_OBSERVATION,
                libsql::params![del.entity_name.clone(), obs],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_delete_relations(
    conn: &Connection,
    relations: Vec<crate::mcp::RelationInput>,
) -> Result<()> {
    let tx = conn.transaction().await?;
    for mut rel in relations {
        rel.from = crate::normalize::normalize_key(&rel.from);
        rel.to = crate::normalize::normalize_key(&rel.to);
        tx.execute(
            crate::constant::SQL_DELETE_RELATION,
            libsql::params![rel.from, rel.to, rel.relation_type],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn mcp_read_graph(conn: &Connection) -> Result<crate::mcp::Graph> {
    let mut entities = Vec::new();
    let mut rows = conn
        .query(crate::constant::SQL_SELECT_ALL_ENTITIES, ())
        .await?;
    while let Some(row) = rows.next().await? {
        let name: String = row.get(0)?;
        let entity_type: String = row.get(1)?;

        let mut obs_rows = conn
            .query(
                crate::constant::SQL_SELECT_OBSERVATIONS_FOR_ENTITY,
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
        .query(crate::constant::SQL_SELECT_ALL_RELATIONS, ())
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
    mcp_search_nodes_with_limit(conn, query, DEFAULT_SEARCH_LIMIT).await
}

pub async fn mcp_search_nodes_with_limit(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<crate::mcp::Graph> {
    let limit = limit.max(1);
    let mut entity_names: Vec<String> = Vec::new();

    // Primary: FTS5 on observation content — porter stemming + BM25 ranking.
    // Wrapped in an async block so any error (invalid syntax, bad token) is
    // caught at the boundary and we fall through to the LIKE path.
    let fts_hits: Vec<String> = async {
        let fts_fetch_limit = limit.saturating_mul(8).max(limit) as i64;
        let mut rows = conn
            .query(
                crate::constant::SQL_SEARCH_OBSERVATIONS_FTS,
                libsql::params![query, fts_fetch_limit],
            )
            .await?;
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
            if entity_names.len() >= limit {
                break;
            }
        }
    }

    // Secondary: LIKE on entity name / type — always runs, catches exact-name
    // lookups and entity types that aren't in observations.
    let pattern = format!("%{}%", query);
    let mut rows = conn
        .query(
            crate::constant::SQL_SEARCH_ENTITIES_LIKE,
            libsql::params![pattern, limit as i64],
        )
        .await?;
    while let Some(row) = rows.next().await? {
        let name: String = row.get(0)?;
        if !entity_names.contains(&name) {
            entity_names.push(name);
            if entity_names.len() >= limit {
                break;
            }
        }
    }

    // Expand neighbors (1-hop)
    let relations = load_relations(conn, &entity_names).await?;
    let mut all_entity_names = entity_names.clone();
    for rel in &relations {
        if !all_entity_names.contains(&rel.from) {
            all_entity_names.push(rel.from.clone());
        }
        if !all_entity_names.contains(&rel.to) {
            all_entity_names.push(rel.to.clone());
        }
    }

    let entities = load_entities(conn, &all_entity_names).await?;

    Ok(crate::mcp::Graph {
        entities,
        relations,
    })
}

pub async fn mcp_open_nodes(conn: &Connection, names: Vec<String>) -> Result<crate::mcp::Graph> {
    let normalized_names: Vec<String> = names
        .into_iter()
        .map(|n| crate::normalize::normalize_key(&n))
        .collect();
    let entities = load_entities(conn, &normalized_names).await?;
    let relations = load_relations(conn, &normalized_names).await?;

    Ok(crate::mcp::Graph {
        entities,
        relations,
    })
}

async fn load_relations(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::mcp::RelationInput>> {
    let mut relations = Vec::new();
    if names.is_empty() {
        return Ok(relations);
    }

    for chunk in names.chunks(400) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = crate::constant::SQL_SELECT_RELATIONS_IN_TEMPLATE.replace("{0}", &placeholders);

        let mut params = Vec::new();
        // Since we use the same array twice in the query logic, we only pass params once, wait!
        // The query "from_entity IN ({0}) OR to_entity IN ({0})" uses the placeholders twice.
        // It's technically better to use different placeholders, but libsql bindings map "?" sequentially.
        // So we need to push the parameters twice!
        for name in chunk {
            params.push(libsql::Value::from(name.clone()));
        }
        for name in chunk {
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

    Ok(relations)
}

async fn load_entities(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::mcp::EntityOutput>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut entity_types = HashMap::new();
    let mut observations: HashMap<String, Vec<String>> = HashMap::new();

    for chunk in names.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");

        // Load entities
        let entity_sql =
            crate::constant::SQL_SELECT_ENTITIES_IN_TEMPLATE.replace("{}", &placeholders);
        let params = chunk
            .iter()
            .cloned()
            .map(libsql::Value::from)
            .collect::<Vec<_>>();

        let mut rows = conn.query(&entity_sql, params.clone()).await?;
        while let Some(row) = rows.next().await? {
            entity_types.insert(row.get::<String>(0)?, row.get::<String>(1)?);
        }

        // Load observations
        let obs_sql =
            crate::constant::SQL_SELECT_OBSERVATIONS_IN_TEMPLATE.replace("{}", &placeholders);

        let mut rows = conn.query(&obs_sql, params).await?;
        while let Some(row) = rows.next().await? {
            observations
                .entry(row.get::<String>(0)?)
                .or_default()
                .push(row.get::<String>(1)?);
        }
    }

    let mut entities = Vec::new();
    for name in names {
        if let Some(entity_type) = entity_types.get(name) {
            entities.push(crate::mcp::EntityOutput {
                name: name.clone(),
                entity_type: entity_type.clone(),
                observations: observations.remove(name).unwrap_or_default(),
            });
        }
    }
    Ok(entities)
}

pub async fn mcp_stats(conn: &Connection) -> Result<(usize, usize, usize)> {
    let mut rows = conn.query(crate::constant::SQL_COUNT_ENTITIES, ()).await?;
    let entities_count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    let mut rows = conn.query(crate::constant::SQL_COUNT_RELATIONS, ()).await?;
    let relations_count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    let mut rows = conn
        .query(crate::constant::SQL_COUNT_OBSERVATIONS, ())
        .await?;
    let observations_count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    Ok((
        entities_count as usize,
        relations_count as usize,
        observations_count as usize,
    ))
}

pub async fn mcp_reset(conn: &Connection) -> Result<()> {
    conn.execute(crate::constant::SQL_DELETE_ALL_RELATIONS, ())
        .await?;
    conn.execute(crate::constant::SQL_DELETE_ALL_OBSERVATIONS, ())
        .await?;
    conn.execute(crate::constant::SQL_DELETE_ALL_ENTITIES, ())
        .await?;
    Ok(())
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
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
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
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
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

    #[tokio::test]
    async fn test_upsert_topic_preserves_created_at() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
        }
        let (_db, conn) = init_db().await.unwrap();

        // Seed a row with a fixed, distinguishable created_at so a reset would
        // be detectable even within the same wall-clock second.
        conn.execute(
            "INSERT INTO topics (id, title, file_path, body, created_at) \
             VALUES ('t1', 'Old Title', '/old', 'old body', '2000-01-01 00:00:00')",
            (),
        )
        .await
        .unwrap();

        // Re-upsert the same id with new content.
        upsert_topic(&conn, "t1", "New Title", "/new", "new body")
            .await
            .unwrap();

        let mut rows = conn
            .query(
                "SELECT title, body, created_at FROM topics WHERE id = 't1'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap().expect("row should exist");
        let title: String = row.get(0).unwrap();
        let body: String = row.get(1).unwrap();
        let created_at: String = row.get(2).unwrap();

        // Update was applied...
        assert_eq!(title, "New Title");
        assert_eq!(body, "new body");
        // ...but created_at must be preserved (INSERT OR REPLACE would reset it).
        assert_eq!(created_at, "2000-01-01 00:00:00");
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
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
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
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        // Entity with no observations — FTS finds nothing, LIKE fallback finds by name
        seed_entity(&conn, "user-preferences", "preference", &[]).await;

        let graph = mcp_search_nodes(&conn, "user-preferences").await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "user-preferences");
    }

    #[tokio::test]
    async fn test_search_nodes_bm25_ordering() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
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
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        // Invalid FTS5 syntax — must not panic, falls back to LIKE gracefully
        let result = mcp_search_nodes(&conn, "AND AND").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_search_nodes_default_limit_and_explicit_limit() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        for i in 0..(DEFAULT_SEARCH_LIMIT + 10) {
            seed_entity(&conn, &format!("entity-{i:03}"), "project", &["commonterm"]).await;
        }

        let default_graph = mcp_search_nodes(&conn, "commonterm").await.unwrap();
        assert_eq!(default_graph.entities.len(), DEFAULT_SEARCH_LIMIT);

        let explicit_graph = mcp_search_nodes_with_limit(&conn, "commonterm", 7)
            .await
            .unwrap();
        assert_eq!(explicit_graph.entities.len(), 7);
    }

    #[tokio::test]
    async fn test_mcp_stats_and_reset() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        // Check empty stats
        let stats = mcp_stats(&conn).await.unwrap();
        assert_eq!(stats, (0, 0, 0));

        // Seed some data
        seed_entity(&conn, "entity1", "project", &["obs1", "obs2"]).await;
        seed_entity(&conn, "entity2", "project", &["obs3"]).await;
        mcp_create_relations(
            &conn,
            vec![crate::mcp::RelationInput {
                from: "entity1".to_string(),
                to: "entity2".to_string(),
                relation_type: "related".to_string(),
            }],
        )
        .await
        .unwrap();

        // Check populated stats
        let stats = mcp_stats(&conn).await.unwrap();
        assert_eq!(stats, (2, 1, 3));

        // Test reset
        mcp_reset(&conn).await.unwrap();

        // Check empty stats again
        let stats = mcp_stats(&conn).await.unwrap();
        assert_eq!(stats, (0, 0, 0));
    }
}
