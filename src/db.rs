use anyhow::Result;
use libsql::{Builder, Connection, Database};
use std::collections::HashMap;
use std::env;

pub const DEFAULT_SEARCH_LIMIT: usize = 100;
pub use crate::constant::ENV_DATABASE_URL;

/// Idempotent migration: if a 0.1.0 database is detected (tables still named
/// `mcp_*`), rename them to the asobi-native `asobi_*` names and repopulate
/// the FTS index. Safe to call on a fresh or already-migrated database — the
/// detection query is a no-op in both cases.
///
/// Ordering:
///   1. Drop old FTS table and triggers (they pin the old content-table name).
///   2. Drop the old index.
///   3. Rename the five base tables (FK refs are updated by SQLite on RENAME).
///   4. Return `true` so the caller knows to repopulate FTS after recreating it.
async fn migrate_mcp_to_asobi(conn: &Connection) -> Result<bool> {
    // Each detection query is scoped in its own block so the read cursor is
    // dropped (statement finalized) before any DDL runs below — an open cursor
    // holds a schema lock and would make ALTER/DROP fail with "table is locked".

    // Check whether the legacy table exists.
    let legacy_exists = {
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='mcp_entities'",
                (),
            )
            .await?;
        rows.next().await?.is_some()
    };
    if !legacy_exists {
        // Fresh db or already migrated — nothing to do.
        return Ok(false);
    }

    // Also verify the target table does NOT exist yet (idempotency guard).
    let already_migrated = {
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='asobi_entities'",
                (),
            )
            .await?;
        rows.next().await?.is_some()
    };
    if already_migrated {
        // Both names present is impossible in normal operation, but if it ever
        // happens treat it as done.
        return Ok(false);
    }

    // Step 1: Drop old triggers — they reference mcp_obs_fts which must also go.
    conn.execute("DROP TRIGGER IF EXISTS mcp_obs_ai", ())
        .await?;
    conn.execute("DROP TRIGGER IF EXISTS mcp_obs_ad", ())
        .await?;
    conn.execute("DROP TRIGGER IF EXISTS mcp_obs_au", ())
        .await?;

    // Step 2: Drop old FTS virtual table (content='mcp_observations' is baked in).
    conn.execute("DROP TABLE IF EXISTS mcp_obs_fts", ()).await?;

    // Step 3: Drop old index (will be recreated with the new name).
    conn.execute("DROP INDEX IF EXISTS idx_mcp_observations_entity_name", ())
        .await?;

    // Step 4: Rename tables. SQLite >= 3.26 updates child FK references when the
    // parent table is renamed, so renaming asobi_entities first is safe.
    conn.execute("ALTER TABLE mcp_entities RENAME TO asobi_entities", ())
        .await?;
    conn.execute(
        "ALTER TABLE mcp_observations RENAME TO asobi_observations",
        (),
    )
    .await?;
    conn.execute("ALTER TABLE mcp_relations RENAME TO asobi_relations", ())
        .await?;
    conn.execute("ALTER TABLE mcp_truths RENAME TO asobi_truths", ())
        .await?;
    // mcp_skills may not exist in every 0.1.0 db; guard it (scoped cursor, as above).
    let has_skills = {
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='mcp_skills'",
                (),
            )
            .await?;
        rows.next().await?.is_some()
    };
    if has_skills {
        conn.execute("ALTER TABLE mcp_skills RENAME TO asobi_skills", ())
            .await?;
    }

    Ok(true)
}

pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::AsobiPaths::resolve();
    if !paths.data_dir.exists() {
        std::fs::create_dir_all(&paths.data_dir)?;
    }

    let db_path = env::var(ENV_DATABASE_URL)
        .unwrap_or_else(|_| paths.db_path().to_str().unwrap().to_string());
    let db = Builder::new_local(&db_path).build().await?;
    let conn = db.connect()?;

    conn.execute(crate::constant::PRAGMA_FOREIGN_KEYS_ON, ())
        .await?;
    // Enable WAL mode for concurrent write support. These pragmas can return a
    // row (e.g. journal_mode), so query + consume. Each cursor is scoped to one
    // loop iteration and dropped before later DDL/migration runs — a lingering
    // read statement would hold a schema lock ("database table is locked").
    for pragma in [
        crate::constant::PRAGMA_JOURNAL_MODE_WAL,
        crate::constant::PRAGMA_SYNCHRONOUS_NORMAL,
        crate::constant::PRAGMA_BUSY_TIMEOUT,
    ] {
        let mut rows = conn.query(pragma, ()).await?;
        let _ = rows.next().await?;
    }

    conn.execute(crate::constant::SCHEMA_CREATE_TOPICS, ())
        .await?;

    // FTS5 for full-text keyword search
    conn.execute(crate::constant::SCHEMA_CREATE_TOPICS_FTS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_SESSIONS, ())
        .await?;

    // Migrate 0.1.0 databases in place before creating new-name tables.
    // Returns true when a migration ran (FTS must be repopulated afterwards).
    let migrated = migrate_mcp_to_asobi(&conn).await?;

    // Graph Tier (Hot) — CREATE IF NOT EXISTS is a no-op on migrated tables.
    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_ENTITIES, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_OBSERVATIONS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_IDX_ASOBI_OBSERVATIONS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_RELATIONS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_TRUTHS, ())
        .await?;

    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_SKILLS, ())
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
    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_OBS_FTS, ())
        .await?;

    // Triggers to keep asobi_obs_fts in sync with asobi_observations
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AI, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AD, ())
        .await?;
    conn.execute(crate::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AU, ())
        .await?;

    // Repopulate FTS after migration: the old mcp_obs_fts was dropped and the
    // new asobi_obs_fts was just created empty. Triggers will keep it in sync
    // going forward; we bulk-insert the existing rows here.
    if migrated {
        conn.execute(
            "INSERT INTO asobi_obs_fts(rowid, content) \
             SELECT rowid, content FROM asobi_observations",
            (),
        )
        .await?;
    }

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

pub async fn delete_topic(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM topics WHERE id = ?1", libsql::params![id])
        .await?;
    Ok(())
}

pub async fn create_entities(
    conn: &Connection,
    entities: Vec<crate::model::EntityInput>,
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

pub async fn add_observations(
    conn: &Connection,
    observations: Vec<crate::model::ObservationInput>,
    limit: usize,
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
        if limit > 0 {
            tx.execute(
                crate::constant::SQL_EVICT_OBSERVATIONS,
                libsql::params![obs_batch.entity_name.clone(), limit as i64],
            )
            .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

pub async fn create_relations(
    conn: &Connection,
    relations: Vec<crate::model::RelationInput>,
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

pub async fn delete_entities(conn: &Connection, names: Vec<String>) -> Result<()> {
    let tx = conn.transaction().await?;
    for name in names {
        let norm_name = crate::normalize::normalize_key(&name);
        tx.execute(
            crate::constant::SQL_DELETE_ENTITY,
            libsql::params![norm_name.clone()],
        )
        .await?;
        tx.execute(
            "DELETE FROM topics WHERE id = ?1",
            libsql::params![norm_name.clone()],
        )
        .await?;
        tx.execute(
            "DELETE FROM chunks WHERE topic_id = ?1",
            libsql::params![norm_name],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn delete_observations(
    conn: &Connection,
    deletions: Vec<crate::model::ObservationDeletion>,
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

pub async fn delete_relations(
    conn: &Connection,
    relations: Vec<crate::model::RelationInput>,
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

pub async fn read_graph(conn: &Connection) -> Result<crate::model::Graph> {
    let mut entity_names = Vec::new();
    let mut rows = conn
        .query(crate::constant::SQL_SELECT_ALL_ENTITIES, ())
        .await?;
    while let Some(row) = rows.next().await? {
        entity_names.push(row.get::<String>(0)?);
    }
    let entities = load_entities_lazy(conn, &entity_names).await?;

    let mut relations = Vec::new();
    let mut rel_rows = conn
        .query(crate::constant::SQL_SELECT_ALL_RELATIONS, ())
        .await?;
    while let Some(row) = rel_rows.next().await? {
        relations.push(crate::model::RelationInput {
            from: row.get(0)?,
            to: row.get(1)?,
            relation_type: row.get(2)?,
        });
    }

    Ok(crate::model::Graph {
        entities,
        relations,
    })
}

pub async fn read_graph_eager(conn: &Connection) -> Result<crate::model::Graph> {
    let mut entity_names = Vec::new();
    let mut rows = conn
        .query(crate::constant::SQL_SELECT_ALL_ENTITIES, ())
        .await?;
    while let Some(row) = rows.next().await? {
        entity_names.push(row.get::<String>(0)?);
    }
    let entities = load_entities_eager(conn, &entity_names).await?;

    let mut relations = Vec::new();
    let mut rel_rows = conn
        .query(crate::constant::SQL_SELECT_ALL_RELATIONS, ())
        .await?;
    while let Some(row) = rel_rows.next().await? {
        relations.push(crate::model::RelationInput {
            from: row.get(0)?,
            to: row.get(1)?,
            relation_type: row.get(2)?,
        });
    }

    Ok(crate::model::Graph {
        entities,
        relations,
    })
}

pub async fn search_nodes(conn: &Connection, query: &str) -> Result<crate::model::Graph> {
    search_nodes_with_limit(conn, query, DEFAULT_SEARCH_LIMIT).await
}

pub async fn search_nodes_with_limit(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> Result<crate::model::Graph> {
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

    let entities = load_entities_lazy(conn, &all_entity_names).await?;

    Ok(crate::model::Graph {
        entities,
        relations,
    })
}

pub async fn open_nodes(conn: &Connection, names: Vec<String>) -> Result<crate::model::Graph> {
    let normalized_names: Vec<String> = names
        .into_iter()
        .map(|n| crate::normalize::normalize_key(&n))
        .collect();
    let entities = load_entities_eager(conn, &normalized_names).await?;
    let relations = load_relations(conn, &normalized_names).await?;

    Ok(crate::model::Graph {
        entities,
        relations,
    })
}

async fn load_relations(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::model::RelationInput>> {
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
            relations.push(crate::model::RelationInput {
                from: row.get(0)?,
                to: row.get(1)?,
                relation_type: row.get(2)?,
            });
        }
    }

    Ok(relations)
}

async fn load_entities_lazy(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::model::EntityOutput>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut entity_types = HashMap::new();
    let mut obs_counts: HashMap<String, usize> = HashMap::new();
    let truths = select_truths(conn, names).await?;

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

        // Load observation counts
        let obs_count_sql = format!(
            "SELECT entity_name, COUNT(*) FROM asobi_observations WHERE entity_name IN ({}) GROUP BY entity_name",
            placeholders
        );
        let mut rows = conn.query(&obs_count_sql, params).await?;
        while let Some(row) = rows.next().await? {
            obs_counts.insert(row.get::<String>(0)?, row.get::<i64>(1)? as usize);
        }
    }

    let mut entities = Vec::new();
    for name in names {
        if let Some(entity_type) = entity_types.get(name) {
            let entity_truths = truths.get(name).cloned().unwrap_or_default();
            let count = obs_counts.get(name).cloned().unwrap_or(0);
            entities.push(crate::model::EntityOutput {
                name: name.clone(),
                entity_type: entity_type.clone(),
                observations: Vec::new(),
                truths: entity_truths,
                observation_count: count,
                body: None,
            });
        }
    }
    Ok(entities)
}

async fn load_entities_eager(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::model::EntityOutput>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut entity_types = HashMap::new();
    let mut observations: HashMap<String, Vec<String>> = HashMap::new();
    let mut skill_bodies: HashMap<String, String> = HashMap::new();
    let truths = select_truths(conn, names).await?;

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

        let mut rows = conn.query(&obs_sql, params.clone()).await?;
        while let Some(row) = rows.next().await? {
            observations
                .entry(row.get::<String>(0)?)
                .or_default()
                .push(row.get::<String>(1)?);
        }

        // Load skill bodies
        let skill_sql =
            crate::constant::SQL_SELECT_SKILL_BODIES_IN_TEMPLATE.replace("{}", &placeholders);
        let mut rows = conn.query(&skill_sql, params).await?;
        while let Some(row) = rows.next().await? {
            skill_bodies.insert(row.get::<String>(0)?, row.get::<String>(1)?);
        }
    }

    let mut entities = Vec::new();
    for name in names {
        if let Some(entity_type) = entity_types.get(name) {
            let entity_truths = truths.get(name).cloned().unwrap_or_default();
            let entity_obs = observations.remove(name).unwrap_or_default();
            let count = entity_obs.len();
            let body = skill_bodies.get(name).cloned();
            entities.push(crate::model::EntityOutput {
                name: name.clone(),
                entity_type: entity_type.clone(),
                observations: entity_obs,
                truths: entity_truths,
                observation_count: count,
                body,
            });
        }
    }
    Ok(entities)
}

pub async fn stats(conn: &Connection) -> Result<(usize, usize, usize)> {
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

pub async fn reset(conn: &Connection) -> Result<()> {
    conn.execute(crate::constant::SQL_DELETE_ALL_RELATIONS, ())
        .await?;
    conn.execute(crate::constant::SQL_DELETE_ALL_OBSERVATIONS, ())
        .await?;
    conn.execute(crate::constant::SQL_DELETE_ALL_ENTITIES, ())
        .await?;
    Ok(())
}

pub async fn truth_upsert(conn: &Connection, entity: &str, key: &str, value: &str) -> Result<()> {
    let norm_entity = crate::normalize::normalize_key(entity);
    conn.execute(
        crate::constant::SQL_UPSERT_TRUTH,
        libsql::params![norm_entity, key, value],
    )
    .await?;
    Ok(())
}

pub async fn truth_delete(conn: &Connection, entity: &str, key: &str) -> Result<()> {
    let norm_entity = crate::normalize::normalize_key(entity);
    conn.execute(
        crate::constant::SQL_DELETE_TRUTH,
        libsql::params![norm_entity, key],
    )
    .await?;
    Ok(())
}

pub async fn select_truths(
    conn: &Connection,
    names: &[impl AsRef<str>],
) -> Result<HashMap<String, std::collections::BTreeMap<String, String>>> {
    let mut results = HashMap::new();
    if names.is_empty() {
        return Ok(results);
    }

    let normalized_names: Vec<String> = names
        .iter()
        .map(|n| crate::normalize::normalize_key(n.as_ref()))
        .filter(|n| !n.is_empty())
        .collect();

    if normalized_names.is_empty() {
        return Ok(results);
    }

    for chunk in normalized_names.chunks(500) {
        let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = crate::constant::SQL_SELECT_TRUTHS_FOR_ENTITIES.replace("{}", &placeholders);
        let params = chunk
            .iter()
            .cloned()
            .map(libsql::Value::from)
            .collect::<Vec<_>>();

        let mut rows = conn.query(&sql, params).await?;
        while let Some(row) = rows.next().await? {
            let entity_name: String = row.get(0)?;
            let key: String = row.get(1)?;
            let value: String = row.get(2)?;
            results
                .entry(entity_name)
                .or_insert_with(std::collections::BTreeMap::new)
                .insert(key, value);
        }
    }

    Ok(results)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillRow {
    pub entity_name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub installed_at: String,
}

pub async fn skill_upsert(
    conn: &Connection,
    entity: &str,
    body: &str,
    source: &str,
    version: &str,
) -> Result<()> {
    let norm_entity = crate::normalize::normalize_key(entity);
    conn.execute(
        crate::constant::SQL_UPSERT_SKILL,
        libsql::params![norm_entity, body, source, version],
    )
    .await?;
    Ok(())
}

pub async fn skill_body(conn: &Connection, entity: &str) -> Result<Option<String>> {
    let norm_entity = crate::normalize::normalize_key(entity);
    let mut rows = conn
        .query(
            crate::constant::SQL_SELECT_SKILL_BODY,
            libsql::params![norm_entity],
        )
        .await?;
    if let Some(row) = rows.next().await? {
        let body: String = row.get(0)?;
        Ok(Some(body))
    } else {
        Ok(None)
    }
}

pub async fn list_skills(conn: &Connection) -> Result<Vec<SkillRow>> {
    let mut rows = conn.query(crate::constant::SQL_LIST_SKILLS, ()).await?;
    let mut results = Vec::new();
    while let Some(row) = rows.next().await? {
        results.push(SkillRow {
            entity_name: row.get(0)?,
            description: row.get(1)?,
            version: row.get(2)?,
            source: row.get(3)?,
            installed_at: row.get(4)?,
        });
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
            "INSERT INTO topics (id, title, file_path) VALUES ('rust-pin', 'Rust Pinning', '.asobi/topics/rust-pinning.md')",
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
        create_entities(
            conn,
            vec![crate::model::EntityInput {
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
        let graph = search_nodes(&conn, "run").await.unwrap();
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

        let graph = search_nodes(&conn, "user-preferences").await.unwrap();
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

        let graph = search_nodes(&conn, "async tokio").await.unwrap();
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
        let result = search_nodes(&conn, "AND AND").await;
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

        let default_graph = search_nodes(&conn, "commonterm").await.unwrap();
        assert_eq!(default_graph.entities.len(), DEFAULT_SEARCH_LIMIT);

        let explicit_graph = search_nodes_with_limit(&conn, "commonterm", 7)
            .await
            .unwrap();
        assert_eq!(explicit_graph.entities.len(), 7);
    }

    #[tokio::test]
    async fn test_stats_and_reset() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        // Check empty stats
        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (0, 0, 0));

        // Seed some data
        seed_entity(&conn, "entity1", "project", &["obs1", "obs2"]).await;
        seed_entity(&conn, "entity2", "project", &["obs3"]).await;
        create_relations(
            &conn,
            vec![crate::model::RelationInput {
                from: "entity1".to_string(),
                to: "entity2".to_string(),
                relation_type: "related".to_string(),
            }],
        )
        .await
        .unwrap();

        // Check populated stats
        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (2, 1, 3));

        // Test reset
        reset(&conn).await.unwrap();

        // Check empty stats again
        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (0, 0, 0));
    }

    #[tokio::test]
    async fn test_truth_upsert_twice_same_key() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;

        truth_upsert(&conn, "test-entity", "version", "1.0.0")
            .await
            .unwrap();
        truth_upsert(&conn, "test-entity", "version", "1.0.1")
            .await
            .unwrap();

        let truths = select_truths(&conn, &["test-entity"]).await.unwrap();
        let entity_truths = truths.get("test-entity").expect("should have truths");
        assert_eq!(entity_truths.len(), 1);
        assert_eq!(entity_truths.get("version").unwrap(), "1.0.1");
    }

    #[tokio::test]
    async fn test_truth_upsert_two_keys() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;

        truth_upsert(&conn, "test-entity", "version", "1.0.0")
            .await
            .unwrap();
        truth_upsert(&conn, "test-entity", "author", "Alice")
            .await
            .unwrap();

        let truths = select_truths(&conn, &["test-entity"]).await.unwrap();
        let entity_truths = truths.get("test-entity").expect("should have truths");
        assert_eq!(entity_truths.len(), 2);
        assert_eq!(entity_truths.get("author").unwrap(), "Alice");
        assert_eq!(entity_truths.get("version").unwrap(), "1.0.0");
    }

    #[tokio::test]
    async fn test_truth_delete() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;

        truth_upsert(&conn, "test-entity", "k1", "v1")
            .await
            .unwrap();
        truth_upsert(&conn, "test-entity", "k2", "v2")
            .await
            .unwrap();

        truth_delete(&conn, "test-entity", "k1").await.unwrap();

        let truths = select_truths(&conn, &["test-entity"]).await.unwrap();
        let entity_truths = truths.get("test-entity").expect("should have truths");
        assert_eq!(entity_truths.len(), 1);
        assert_eq!(entity_truths.get("k2").unwrap(), "v2");
    }

    #[tokio::test]
    async fn test_delete_entities_cascades_truths() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;
        truth_upsert(&conn, "test-entity", "k1", "v1")
            .await
            .unwrap();

        delete_entities(&conn, vec!["test-entity".to_string()])
            .await
            .unwrap();

        // Check if the truth was deleted.
        let mut rows = conn
            .query("SELECT COUNT(*) FROM asobi_truths", ())
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_observation_limit_evicts_oldest() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;

        let inputs = vec![crate::model::ObservationInput {
            entity_name: "test-entity".to_string(),
            contents: vec![
                "obs1".to_string(),
                "obs2".to_string(),
                "obs3".to_string(),
                "obs4".to_string(),
                "obs5".to_string(),
            ],
        }];
        add_observations(&conn, inputs, 3).await.unwrap();

        let graph = open_nodes(&conn, vec!["test-entity".to_string()])
            .await
            .unwrap();
        let entity = &graph.entities[0];
        let mut obs = entity.observations.clone();
        obs.sort();
        assert_eq!(
            obs,
            vec!["obs3".to_string(), "obs4".to_string(), "obs5".to_string(),]
        );
    }

    #[tokio::test]
    async fn test_observation_limit_zero_is_unbounded() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &[]).await;

        let inputs = vec![crate::model::ObservationInput {
            entity_name: "test-entity".to_string(),
            contents: vec![
                "obs1".to_string(),
                "obs2".to_string(),
                "obs3".to_string(),
                "obs4".to_string(),
                "obs5".to_string(),
            ],
        }];
        add_observations(&conn, inputs, 0).await.unwrap();

        let graph = open_nodes(&conn, vec!["test-entity".to_string()])
            .await
            .unwrap();
        let entity = &graph.entities[0];
        assert_eq!(entity.observations.len(), 5);
    }

    #[tokio::test]
    async fn test_lazy_read_graph_and_search_nodes() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "test-entity", "concept", &["obs1", "obs2"]).await;
        truth_upsert(&conn, "test-entity", "k1", "v1")
            .await
            .unwrap();

        // 1. Test read-graph (should be lazy)
        let graph_read = read_graph(&conn).await.unwrap();
        let entity_read = &graph_read.entities[0];
        assert!(entity_read.observations.is_empty());
        assert_eq!(entity_read.observation_count, 2);
        assert_eq!(entity_read.truths.len(), 1);
        assert_eq!(entity_read.truths.get("k1").unwrap(), "v1");

        // 2. Test search-nodes (should be lazy)
        let graph_search = search_nodes(&conn, "test").await.unwrap();
        let entity_search = &graph_search.entities[0];
        assert!(entity_search.observations.is_empty());
        assert_eq!(entity_search.observation_count, 2);
        assert_eq!(entity_search.truths.len(), 1);
        assert_eq!(entity_search.truths.get("k1").unwrap(), "v1");

        // 3. Test open-nodes (should be eager)
        let graph_open = open_nodes(&conn, vec!["test-entity".to_string()])
            .await
            .unwrap();
        let entity_open = &graph_open.entities[0];
        assert_eq!(entity_open.observations.len(), 2);
        assert_eq!(entity_open.observation_count, 2);
        assert_eq!(entity_open.truths.len(), 1);
        assert_eq!(entity_open.truths.get("k1").unwrap(), "v1");
    }

    #[tokio::test]
    async fn test_skill_storage_and_open_nodes() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "skill:test-skill", "skill", &[]).await;
        truth_upsert(&conn, "skill:test-skill", "description", "my test skill")
            .await
            .unwrap();

        // 1. Upsert skill
        skill_upsert(
            &conn,
            "skill:test-skill",
            "body content 1",
            "source-1",
            "1.0.0",
        )
        .await
        .unwrap();

        // 2. open-nodes should return the body
        let graph = open_nodes(&conn, vec!["skill:test-skill".to_string()])
            .await
            .unwrap();
        let entity = &graph.entities[0];
        assert_eq!(entity.body.as_deref(), Some("body content 1"));

        // 3. read-graph and search-nodes should NOT return the body
        let graph_read = read_graph(&conn).await.unwrap();
        assert!(graph_read.entities[0].body.is_none());

        let graph_search = search_nodes(&conn, "skill").await.unwrap();
        assert!(graph_search.entities[0].body.is_none());

        // 4. Second upsert should replace the body
        skill_upsert(
            &conn,
            "skill:test-skill",
            "body content 2",
            "source-1",
            "1.0.1",
        )
        .await
        .unwrap();
        let graph_2 = open_nodes(&conn, vec!["skill:test-skill".to_string()])
            .await
            .unwrap();
        assert_eq!(graph_2.entities[0].body.as_deref(), Some("body content 2"));

        // 5. list_skills should list name + description + version + source + installed_at
        let skills = list_skills(&conn).await.unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].entity_name, "skill:test-skill");
        assert_eq!(skills[0].description, "my test skill");
        assert_eq!(skills[0].version, "1.0.1");
        assert_eq!(skills[0].source, "source-1");

        // 6. delete-entities cascades skills
        delete_entities(&conn, vec!["skill:test-skill".to_string()])
            .await
            .unwrap();
        let body_after = skill_body(&conn, "skill:test-skill").await.unwrap();
        assert!(body_after.is_none());

        let count_skills: i64 = conn
            .query("SELECT COUNT(*) FROM asobi_skills", ())
            .await
            .unwrap()
            .next()
            .await
            .unwrap()
            .unwrap()
            .get(0)
            .unwrap();
        assert_eq!(count_skills, 0);
    }

    /// Round-trip migration test: build an old-schema (mcp_*) database by hand,
    /// run init_db against it, then verify that:
    ///   1. Data is accessible under asobi_* names.
    ///   2. FTS search works on the migrated observations.
    ///   3. A second init_db call is a no-op (idempotent).
    ///
    /// Uses a real temp file — NOT :memory: — because migration touches the WAL
    /// and FTS external-content table, both of which behave differently in-process.
    #[tokio::test]
    async fn test_migration_mcp_to_asobi_round_trip() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("legacy.db");
        unsafe {
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        // --- Phase 1: Build a legacy 0.1.0 database by hand ---
        // We use the `sqlite3` system CLI (always available on Linux/macOS) to
        // create the legacy database in a subprocess. This guarantees the file
        // handle is fully released before Phase 2 opens it — no threading
        // conflict with libSQL, no async WAL residue.
        let sql = "
            PRAGMA foreign_keys = ON;
            CREATE TABLE mcp_entities (
                name        TEXT PRIMARY KEY,
                entity_type TEXT NOT NULL,
                created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE mcp_observations (
                id          TEXT PRIMARY KEY,
                entity_name TEXT NOT NULL,
                content     TEXT NOT NULL,
                created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
            );
            CREATE TABLE mcp_relations (
                from_entity   TEXT NOT NULL,
                to_entity     TEXT NOT NULL,
                relation_type TEXT NOT NULL,
                PRIMARY KEY (from_entity, to_entity, relation_type),
                FOREIGN KEY (from_entity) REFERENCES mcp_entities(name) ON DELETE CASCADE,
                FOREIGN KEY (to_entity)   REFERENCES mcp_entities(name) ON DELETE CASCADE
            );
            CREATE TABLE mcp_truths (
                entity_name TEXT NOT NULL,
                key         TEXT NOT NULL,
                value       TEXT NOT NULL,
                PRIMARY KEY (entity_name, key),
                FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
            );
            CREATE TABLE mcp_skills (
                entity_name  TEXT PRIMARY KEY,
                body         TEXT NOT NULL,
                source       TEXT NOT NULL,
                version      TEXT NOT NULL,
                installed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
            );
            CREATE VIRTUAL TABLE mcp_obs_fts
                USING fts5(content, content='mcp_observations', content_rowid='rowid', tokenize='porter unicode61');
            CREATE TRIGGER mcp_obs_ai AFTER INSERT ON mcp_observations BEGIN
                INSERT INTO mcp_obs_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            INSERT INTO mcp_entities (name, entity_type) VALUES ('legacy-entity', 'concept');
            INSERT INTO mcp_observations (id, entity_name, content)
                VALUES ('obs-1', 'legacy-entity', 'the quick brown fox');
            INSERT INTO mcp_truths (entity_name, key, value)
                VALUES ('legacy-entity', 'status', 'active');
        ";
        let status = std::process::Command::new("sqlite3")
            .arg(db_path.to_str().unwrap())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(sql.as_bytes())
                    .unwrap();
                child.wait()
            })
            .expect("sqlite3 must be installed for this test");
        assert!(status.success(), "sqlite3 failed to build legacy database");

        // --- Phase 2: Run init_db (migration fires) ---
        let (_db, conn) = init_db().await.unwrap();

        // Data must exist under the new table names.
        let mut rows = conn
            .query(
                "SELECT name, entity_type FROM asobi_entities WHERE name = 'legacy-entity'",
                (),
            )
            .await
            .unwrap();
        let row = rows
            .next()
            .await
            .unwrap()
            .expect("entity must exist after migration");
        assert_eq!(row.get::<String>(0).unwrap(), "legacy-entity");
        assert_eq!(row.get::<String>(1).unwrap(), "concept");

        // Observation must be accessible.
        let mut rows = conn
            .query(
                "SELECT content FROM asobi_observations WHERE entity_name = 'legacy-entity'",
                (),
            )
            .await
            .unwrap();
        let obs_row = rows.next().await.unwrap().expect("observation must exist");
        assert_eq!(obs_row.get::<String>(0).unwrap(), "the quick brown fox");

        // Truth must be accessible.
        let mut rows = conn
            .query(
                "SELECT value FROM asobi_truths WHERE entity_name = 'legacy-entity' AND key = 'status'",
                (),
            )
            .await
            .unwrap();
        let truth_row = rows.next().await.unwrap().expect("truth must exist");
        assert_eq!(truth_row.get::<String>(0).unwrap(), "active");

        // FTS search must find the observation after FTS repopulation.
        let graph = search_nodes(&conn, "quick").await.unwrap();
        assert_eq!(
            graph.entities.len(),
            1,
            "FTS should find legacy-entity via repopulated asobi_obs_fts"
        );
        assert_eq!(graph.entities[0].name, "legacy-entity");

        // Old mcp_entities table must be gone.
        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='mcp_entities'",
                (),
            )
            .await
            .unwrap();
        assert!(
            rows.next().await.unwrap().is_none(),
            "mcp_entities must be absent after migration"
        );

        // --- Phase 3: Second init_db must be idempotent (no error) ---
        let (_db2, conn2) = init_db().await.unwrap();

        // Data still intact after second init.
        let mut rows = conn2
            .query("SELECT COUNT(*) FROM asobi_entities", ())
            .await
            .unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 1, "second init_db must not drop data");
    }
}
