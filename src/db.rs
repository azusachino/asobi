use anyhow::{Context, Result};
use libsql::{Builder, Connection, Database};
use std::collections::HashMap;
use std::env;

pub const DEFAULT_SEARCH_LIMIT: usize = 100;
pub use crate::constant::{ENV_BUSY_TIMEOUT, ENV_DATABASE_URL};

pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::AsobiPaths::resolve();
    if !paths.data_dir.exists() {
        std::fs::create_dir_all(&paths.data_dir)
            .with_context(|| format!(
                "failed to create database directory at '{}'. Hint: run 'asobi init --local' or set ASOBI_HOME to a writable directory.",
                paths.data_dir.display()
            ))?;
    }

    let db_path = env::var(ENV_DATABASE_URL)
        .unwrap_or_else(|_| paths.db_path().to_str().unwrap().to_string());
    let db = Builder::new_local(&db_path)
        .build()
        .await
        .with_context(|| format!(
            "failed to build/open database file at '{}'. Hint: run 'asobi init --local' or set ASOBI_HOME to a writable directory.",
            db_path
        ))?;
    let conn = db.connect()?;

    let timeout_ms = env::var(ENV_BUSY_TIMEOUT)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15000);
    let busy_timeout_pragma = format!("PRAGMA busy_timeout = {}", timeout_ms);

    conn.execute(crate::constant::PRAGMA_FOREIGN_KEYS_ON, ())
        .await?;
    // Enable WAL mode for concurrent write support. These pragmas can return a
    // row (e.g. journal_mode), so query + consume. Each cursor is scoped to one
    // loop iteration and dropped before later DDL/migration runs — a lingering
    // read statement would hold a schema lock ("database table is locked").
    for pragma in [
        crate::constant::PRAGMA_JOURNAL_MODE_WAL,
        crate::constant::PRAGMA_SYNCHRONOUS_NORMAL,
        &busy_timeout_pragma,
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

    // Graph Tier (Hot) — CREATE IF NOT EXISTS is a no-op on migrated tables.
    conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_ENTITIES, ())
        .await?;

    let mut needs_migration = false;
    {
        let mut rows = conn
            .query("PRAGMA table_info(asobi_observations)", ())
            .await?;
        while let Some(row) = rows.next().await? {
            let col_name: String = row.get(1)?;
            let col_type: String = row.get(2)?;
            if col_name == "id" && col_type.to_uppercase() == "TEXT" {
                needs_migration = true;
                break;
            }
        }
    }

    if needs_migration {
        tracing::info!(
            "Migrating asobi_observations 'id' column from TEXT (UUID) to AUTOINCREMENT INTEGER..."
        );
        conn.execute("PRAGMA foreign_keys = OFF;", ()).await?;
        let tx = conn.transaction().await?;
        tx.execute(
            "ALTER TABLE asobi_observations RENAME TO asobi_observations_old",
            (),
        )
        .await?;
        tx.execute(crate::constant::SCHEMA_CREATE_ASOBI_OBSERVATIONS, ())
            .await?;
        tx.execute(
            "INSERT INTO asobi_observations (entity_name, content, created_at) \
             SELECT entity_name, content, created_at FROM asobi_observations_old ORDER BY created_at, rowid",
            ()
        ).await?;
        tx.execute("DROP TABLE asobi_observations_old", ()).await?;
        tx.commit().await?;
        conn.execute("PRAGMA foreign_keys = ON;", ()).await?;
    } else {
        conn.execute(crate::constant::SCHEMA_CREATE_ASOBI_OBSERVATIONS, ())
            .await?;
    }

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
                libsql::params![ent.name.clone(), obs],
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
                libsql::params![obs_batch.entity_name.clone(), content],
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

pub async fn delete_observation_by_id(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        crate::constant::SQL_DELETE_OBSERVATION_BY_ID,
        libsql::params![id],
    )
    .await?;
    Ok(())
}

pub async fn update_observation_by_id(conn: &Connection, id: i64, new_content: &str) -> Result<()> {
    conn.execute(
        crate::constant::SQL_UPDATE_OBSERVATION_BY_ID,
        libsql::params![id, new_content],
    )
    .await?;
    Ok(())
}

pub async fn update_observation(
    conn: &Connection,
    entity_name: &str,
    old_content: &str,
    new_content: &str,
) -> Result<()> {
    let norm_name = crate::normalize::normalize_key(entity_name);
    conn.execute(
        crate::constant::SQL_UPDATE_OBSERVATION,
        libsql::params![norm_name, old_content, new_content],
    )
    .await?;
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
    search_nodes_with_limit(conn, query, DEFAULT_SEARCH_LIMIT, &[]).await
}

pub async fn search_nodes_with_limit(
    conn: &Connection,
    query: &str,
    limit: usize,
    filters: &[(String, String)],
) -> Result<crate::model::Graph> {
    let limit = limit.max(1);
    let mut entity_names: Vec<String> = Vec::new();

    let mut filtered_names = std::collections::HashSet::new();
    if !filters.is_empty() {
        let mut sql = "SELECT entity_name FROM asobi_truths WHERE ".to_string();
        let mut params = Vec::new();
        for (i, (k, v)) in filters.iter().enumerate() {
            if i > 0 {
                sql.push_str(" OR ");
            }
            sql.push_str(&format!(
                "(key = ?{} AND value = ?{})",
                i * 2 + 1,
                i * 2 + 2
            ));
            params.push(libsql::Value::from(k.clone()));
            params.push(libsql::Value::from(v.clone()));
        }
        sql.push_str(" GROUP BY entity_name HAVING COUNT(DISTINCT key) = ?");
        params.push(libsql::Value::from(filters.len() as i64));

        let mut rows = conn.query(&sql, libsql::params_from_iter(params)).await?;
        while let Some(row) = rows.next().await? {
            filtered_names.insert(row.get::<String>(0)?);
        }

        if filtered_names.is_empty() {
            return Ok(crate::model::Graph {
                entities: vec![],
                relations: vec![],
            });
        }
    }

    if query.trim().is_empty() {
        if !filters.is_empty() {
            entity_names = filtered_names.into_iter().take(limit).collect();
        } else {
            let mut rows = conn
                .query(
                    "SELECT name FROM asobi_entities LIMIT ?1",
                    libsql::params![limit as i64],
                )
                .await?;
            while let Some(row) = rows.next().await? {
                entity_names.push(row.get(0)?);
            }
        }
    } else {
        // Primary: FTS5 on observation content
        let fts_hits: Vec<String> = async {
            let fts_fetch_limit = if filters.is_empty() {
                limit.saturating_mul(8).max(limit) as i64
            } else {
                5000
            };
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

        let mut seen_names = std::collections::HashSet::new();
        for name in fts_hits {
            if !filters.is_empty() && !filtered_names.contains(&name) {
                continue;
            }
            if seen_names.insert(name.clone()) {
                entity_names.push(name);
                if entity_names.len() >= limit {
                    break;
                }
            }
        }

        if entity_names.len() < limit {
            // Secondary: LIKE on entity name / type
            let pattern = format!("%{}%", query);
            let like_limit = if filters.is_empty() {
                limit as i64
            } else {
                5000
            };
            let mut rows = conn
                .query(
                    crate::constant::SQL_SEARCH_ENTITIES_LIKE,
                    libsql::params![pattern, like_limit],
                )
                .await?;
            while let Some(row) = rows.next().await? {
                let name: String = row.get(0)?;
                if !filters.is_empty() && !filtered_names.contains(&name) {
                    continue;
                }
                if seen_names.insert(name.clone()) {
                    entity_names.push(name);
                    if entity_names.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    // Expand neighbors (1-hop)
    let relations = load_relations(conn, &entity_names).await?;
    let mut all_entity_names = entity_names.clone();
    let mut seen_all = entity_names
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    for rel in &relations {
        if seen_all.insert(rel.from.clone()) {
            all_entity_names.push(rel.from.clone());
        }
        if seen_all.insert(rel.to.clone()) {
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
    open_nodes_detailed(conn, names, false, &[]).await
}

pub async fn open_nodes_detailed(
    conn: &Connection,
    names: Vec<String>,
    with_ids: bool,
    expand_relations: &[String],
) -> Result<crate::model::Graph> {
    let mut normalized_names: Vec<String> = names
        .into_iter()
        .map(|n| crate::normalize::normalize_key(&n))
        .collect();

    let mut relations = load_relations(conn, &normalized_names).await?;
    if !expand_relations.is_empty() {
        let mut extra_names = std::collections::HashSet::new();
        for rel in &relations {
            if expand_relations.contains(&rel.relation_type) {
                if normalized_names.contains(&rel.from) {
                    extra_names.insert(rel.to.clone());
                }
                if normalized_names.contains(&rel.to) {
                    extra_names.insert(rel.from.clone());
                }
            }
        }
        for name in extra_names {
            if !normalized_names.contains(&name) {
                normalized_names.push(name);
            }
        }
        relations = load_relations(conn, &normalized_names).await?;
    }

    let entities = load_entities_eager_detailed(conn, &normalized_names, with_ids).await?;

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
                observations_detailed: None,
            });
        }
    }
    Ok(entities)
}

async fn load_entities_eager(
    conn: &Connection,
    names: &[String],
) -> Result<Vec<crate::model::EntityOutput>> {
    load_entities_eager_detailed(conn, names, false).await
}

async fn load_entities_eager_detailed(
    conn: &Connection,
    names: &[String],
    with_ids: bool,
) -> Result<Vec<crate::model::EntityOutput>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut entity_types = HashMap::new();
    let mut observations: HashMap<String, Vec<String>> = HashMap::new();
    let mut detailed_obs: HashMap<String, Vec<crate::model::DetailedObservation>> = HashMap::new();
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

        let obs_sql =
            crate::constant::SQL_SELECT_OBSERVATIONS_IN_TEMPLATE.replace("{}", &placeholders);
        let mut rows = conn.query(&obs_sql, params.clone()).await?;
        while let Some(row) = rows.next().await? {
            let id = row.get::<i64>(0)?;
            let entity_name = row.get::<String>(1)?;
            let content = row.get::<String>(2)?;

            observations
                .entry(entity_name.clone())
                .or_default()
                .push(content.clone());

            if with_ids {
                detailed_obs
                    .entry(entity_name)
                    .or_default()
                    .push(crate::model::DetailedObservation { id, content });
            }
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
            let observations_detailed = if with_ids {
                Some(detailed_obs.remove(name).unwrap_or_default())
            } else {
                None
            };
            entities.push(crate::model::EntityOutput {
                name: name.clone(),
                entity_type: entity_type.clone(),
                observations: entity_obs,
                truths: entity_truths,
                observation_count: count,
                body,
                observations_detailed,
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

pub async fn stats_per_entity(conn: &Connection) -> Result<Vec<(String, usize)>> {
    let mut results = Vec::new();
    let mut rows = conn
        .query(
            "SELECT entity_name, COUNT(*) as c FROM asobi_observations GROUP BY entity_name ORDER BY c DESC",
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        results.push((row.get::<String>(0)?, row.get::<i64>(1)? as usize));
    }
    Ok(results)
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

        let explicit_graph = search_nodes_with_limit(&conn, "commonterm", 7, &[])
            .await
            .unwrap();
        assert_eq!(explicit_graph.entities.len(), 7);
    }

    #[tokio::test]
    async fn test_search_nodes_with_where_filters() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        // Seed some entities
        seed_entity(&conn, "task-1", "task", &["fix bug"]).await;
        truth_upsert(&conn, "task-1", "status", "READY")
            .await
            .unwrap();
        truth_upsert(&conn, "task-1", "priority", "high")
            .await
            .unwrap();

        seed_entity(&conn, "task-2", "task", &["fix crash"]).await;
        truth_upsert(&conn, "task-2", "status", "BLOCKED")
            .await
            .unwrap();
        truth_upsert(&conn, "task-2", "priority", "high")
            .await
            .unwrap();

        seed_entity(&conn, "task-3", "task", &["write test"]).await;
        truth_upsert(&conn, "task-3", "status", "READY")
            .await
            .unwrap();
        truth_upsert(&conn, "task-3", "priority", "low")
            .await
            .unwrap();

        // 1. Search with status=READY
        let g1 = search_nodes_with_limit(
            &conn,
            "",
            10,
            &[("status".to_string(), "READY".to_string())],
        )
        .await
        .unwrap();
        let names1: std::collections::HashSet<_> =
            g1.entities.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names1.len(), 2);
        assert!(names1.contains("task-1"));
        assert!(names1.contains("task-3"));

        // 2. Search status=READY and priority=high
        let g2 = search_nodes_with_limit(
            &conn,
            "",
            10,
            &[
                ("status".to_string(), "READY".to_string()),
                ("priority".to_string(), "high".to_string()),
            ],
        )
        .await
        .unwrap();
        let names2: std::collections::HashSet<_> =
            g2.entities.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names2.len(), 1);
        assert!(names2.contains("task-1"));

        // 3. Search status=READY with a query term "test"
        let g3 = search_nodes_with_limit(
            &conn,
            "test",
            10,
            &[("status".to_string(), "READY".to_string())],
        )
        .await
        .unwrap();
        let names3: std::collections::HashSet<_> =
            g3.entities.iter().map(|e| e.name.clone()).collect();
        assert_eq!(names3.len(), 1);
        assert!(names3.contains("task-3"));
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

        // 2. Test search (should be lazy)
        let graph_search = search_nodes(&conn, "test").await.unwrap();
        let entity_search = &graph_search.entities[0];
        assert!(entity_search.observations.is_empty());
        assert_eq!(entity_search.observation_count, 2);
        assert_eq!(entity_search.truths.len(), 1);
        assert_eq!(entity_search.truths.get("k1").unwrap(), "v1");

        // 3. Test show (should be eager)
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
    async fn test_skill_storage_and_show() {
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

        // 2. show should return the body
        let graph = open_nodes(&conn, vec!["skill:test-skill".to_string()])
            .await
            .unwrap();
        let entity = &graph.entities[0];
        assert_eq!(entity.body.as_deref(), Some("body content 1"));

        // 3. graph and search should NOT return the body
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
}
