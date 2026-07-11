use anyhow::{Context, Result};
use libsql::{Connection, Database};
use std::collections::HashMap;
use std::env;

pub const DEFAULT_SEARCH_LIMIT: usize = 100;
pub const DEFAULT_DATABASE_FILENAME: &str = "asobi.db";
pub use crate::storage::libsql::constant::ENV_DATABASE_URL;

// v2: document embedding model changed to gte-base-en-v1.5, so the `chunks`
// embedding column moved from F32_BLOB(384) to F32_BLOB(768). Old chunks are
// re-ingestable and incompatible with the new model, so the migration drops and
// recreates the table (see the setup block below).
pub const SCHEMA_VERSION: i64 = 2;

pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::AsobiPaths::resolve();
    let db_path = env::var(ENV_DATABASE_URL).unwrap_or_else(|_| {
        paths
            .data_dir
            .join(DEFAULT_DATABASE_FILENAME)
            .to_str()
            .unwrap()
            .to_string()
    });
    init_db_at(std::path::Path::new(&db_path)).await
}

pub(crate) async fn init_db_at(db_path: &std::path::Path) -> Result<(Database, Connection)> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create database directory at '{}'. Hint: run 'asobi init --local' or set ASOBI_HOME to a writable directory.",
                parent.display()
            )
        })?;
    }

    let (db, conn) = crate::storage::libsql::tx::open_local(db_path)
        .await
        .with_context(|| format!(
            "failed to build/open database file at '{}'. Hint: run 'asobi init --local' or set ASOBI_HOME to a writable directory.",
            db_path.display()
        ))?;

    let timeout_ms = env::var(crate::storage::libsql::constant::ENV_BUSY_TIMEOUT)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(15000);
    let busy_timeout_pragma = format!("PRAGMA busy_timeout = {}", timeout_ms);

    // Apply basic connection pragmas needed for every connection.
    // Set busy timeout FIRST so subsequent pragmas and queries respect it.
    {
        let mut rows = conn.query(&busy_timeout_pragma, ()).await?;
        let _ = rows.next().await?;
    }

    for pragma in [
        crate::storage::libsql::constant::PRAGMA_FOREIGN_KEYS_ON,
        crate::storage::libsql::constant::PRAGMA_SYNCHRONOUS_NORMAL,
    ] {
        let mut rows = conn.query(pragma, ()).await?;
        let _ = rows.next().await?;
    }

    // Query PRAGMA user_version and check if it equals SCHEMA_VERSION.
    // If it matches, immediately return Ok((db, conn)), skipping all DDL and schema creation / migration logic!
    let current_version = {
        let mut rows = conn.query("PRAGMA user_version", ()).await?;
        if let Some(row) = rows.next().await? {
            row.get::<i64>(0)?
        } else {
            0
        }
    };

    if current_version == SCHEMA_VERSION {
        return Ok((db, conn));
    }

    // journal_mode cannot be changed inside a transaction — set it up front.
    let journal_mode = env::var(crate::storage::libsql::constant::ENV_JOURNAL_MODE)
        .unwrap_or_else(|_| "WAL".to_string())
        .to_uppercase();
    let journal_mode_pragma = format!("PRAGMA journal_mode = {}", journal_mode);

    let run_journal_mode = async {
        {
            let mut rows = conn.query(&journal_mode_pragma, ()).await?;
            let _ = rows.next().await?;
        }
        Ok::<(), libsql::Error>(())
    };

    if let Err(e) = run_journal_mode.await {
        tracing::warn!(
            "Failed to set journal_mode to '{}': {:?}. Falling back to DELETE.",
            journal_mode,
            e
        );
        let mut rows = conn.query("PRAGMA journal_mode = DELETE", ()).await?;
        let _ = rows.next().await?;
    }

    // Determine if migration is needed before starting transaction to allow setting foreign keys accordingly.
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
        let mut rows = conn.query("PRAGMA foreign_keys = OFF", ()).await?;
        let _ = rows.next().await?;
    }

    // Start an immediate transaction.
    conn.execute("BEGIN IMMEDIATE", ()).await?;

    let run_setup = async {
        let current_version = {
            let mut rows = conn.query("PRAGMA user_version", ()).await?;
            if let Some(row) = rows.next().await? {
                row.get::<i64>(0)?
            } else {
                0
            }
        };

        if current_version == SCHEMA_VERSION {
            return Ok(());
        }

        conn.execute(crate::storage::libsql::constant::SCHEMA_CREATE_TOPICS, ())
            .await?;

        // FTS5 for full-text keyword search
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TOPICS_FTS,
            (),
        )
        .await?;

        conn.execute(crate::storage::libsql::constant::SCHEMA_CREATE_SESSIONS, ())
            .await?;

        // Graph Tier (Hot) — CREATE IF NOT EXISTS is a no-op on migrated tables.
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_ENTITIES,
            (),
        )
        .await?;

        if needs_migration {
            tracing::info!(
                "Migrating asobi_observations 'id' column from TEXT (UUID) to AUTOINCREMENT INTEGER..."
            );
            conn.execute(
                "ALTER TABLE asobi_observations RENAME TO asobi_observations_old",
                (),
            )
            .await?;
            conn.execute(
                crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_OBSERVATIONS,
                (),
            )
            .await?;
            conn.execute(
                "INSERT INTO asobi_observations (entity_name, content, created_at) \
                 SELECT entity_name, content, created_at FROM asobi_observations_old ORDER BY created_at, rowid",
                ()
            ).await?;
            conn.execute("DROP TABLE asobi_observations_old", ())
                .await?;
        } else {
            conn.execute(
                crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_OBSERVATIONS,
                (),
            )
            .await?;
        }

        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_IDX_ASOBI_OBSERVATIONS,
            (),
        )
        .await?;

        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_RELATIONS,
            (),
        )
        .await?;

        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_TRUTHS,
            (),
        )
        .await?;

        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_SKILLS,
            (),
        )
        .await?;

        // Document Tier (Vectors). The embedding column is dimension-typed
        // (F32_BLOB(768)); a pre-v2 database has a 384-wide column, so drop any
        // existing chunks table (and its dependent vector index) and recreate it
        // at the new dimension. Chunks are a rebuildable cache — the user
        // re-ingests. On a fresh database this DROP is a no-op.
        conn.execute("DROP TABLE IF EXISTS chunks", ()).await?;
        conn.execute(crate::storage::libsql::constant::SCHEMA_CREATE_CHUNKS, ())
            .await?;

        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_IDX_CHUNKS_TOPIC_ID,
            (),
        )
        .await?;

        // Vector index - metric=cosine is default
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_IDX_CHUNKS_VECTOR,
            (),
        )
        .await?;

        // Triggers to keep topics_fts in sync with topics
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AI,
            (),
        )
        .await?;
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AD,
            (),
        )
        .await?;
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_TOPICS_AU,
            (),
        )
        .await?;

        // FTS5 for graph observation search (porter stemming, BM25 ranking)
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_ASOBI_OBS_FTS,
            (),
        )
        .await?;

        // Triggers to keep asobi_obs_fts in sync with asobi_observations
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AI,
            (),
        )
        .await?;
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AD,
            (),
        )
        .await?;
        conn.execute(
            crate::storage::libsql::constant::SCHEMA_CREATE_TRIGGER_ASOBI_OBS_AU,
            (),
        )
        .await?;

        if needs_migration {
            conn.execute(
                "INSERT INTO asobi_obs_fts(asobi_obs_fts) VALUES('rebuild')",
                (),
            )
            .await?;
        }

        let version_pragma = format!("PRAGMA user_version = {}", SCHEMA_VERSION);
        conn.execute(&version_pragma, ()).await?;

        Ok::<(), anyhow::Error>(())
    };

    match run_setup.await {
        Ok(()) => {
            conn.execute("COMMIT", ()).await?;
            if needs_migration {
                let mut rows = conn.query("PRAGMA foreign_keys = ON", ()).await?;
                let _ = rows.next().await?;
            }
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", ()).await;
            if needs_migration {
                let mut rows = conn.query("PRAGMA foreign_keys = ON", ()).await?;
                let _ = rows.next().await?;
            }
            return Err(e);
        }
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
            crate::storage::libsql::constant::SQL_SEARCH_FTS,
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
        crate::storage::libsql::constant::SQL_UPSERT_TOPIC,
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
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let entities = entities.clone();
        Box::pin(async move {
            for mut ent in entities {
                ent.name = crate::normalize::normalize_key(&ent.name);
                let inserted = tx
                    .execute(
                        crate::storage::libsql::constant::SQL_INSERT_ENTITY,
                        libsql::params![ent.name.clone(), ent.entity_type],
                    )
                    .await?;
                if inserted == 1 {
                    for obs in ent.observations {
                        tx.execute(
                            crate::storage::libsql::constant::SQL_INSERT_OBSERVATION,
                            libsql::params![ent.name.clone(), obs],
                        )
                        .await?;
                    }
                }
            }
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn add_observations(
    conn: &Connection,
    observations: Vec<crate::model::ObservationInput>,
    limit: usize,
) -> Result<()> {
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let observations = observations.clone();
        Box::pin(async move {
            for mut obs_batch in observations {
                obs_batch.entity_name = crate::normalize::normalize_key(&obs_batch.entity_name);
                for content in obs_batch.contents {
                    tx.execute(
                        crate::storage::libsql::constant::SQL_INSERT_OBSERVATION,
                        libsql::params![obs_batch.entity_name.clone(), content],
                    )
                    .await?;
                }
                if limit > 0 {
                    tx.execute(
                        crate::storage::libsql::constant::SQL_EVICT_OBSERVATIONS,
                        libsql::params![obs_batch.entity_name.clone(), limit as i64],
                    )
                    .await?;
                }
            }
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn create_relations(
    conn: &Connection,
    relations: Vec<crate::model::RelationInput>,
) -> Result<()> {
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let relations = relations.clone();
        Box::pin(async move {
            for mut rel in relations {
                rel.from = crate::normalize::normalize_key(&rel.from);
                rel.to = crate::normalize::normalize_key(&rel.to);
                tx.execute(
                    crate::storage::libsql::constant::SQL_INSERT_RELATION,
                    libsql::params![rel.from, rel.to, rel.relation_type],
                )
                .await?;
            }
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn delete_entities(conn: &Connection, names: Vec<String>) -> Result<()> {
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let names = names.clone();
        Box::pin(async move {
            for name in names {
                let norm_name = crate::normalize::normalize_key(&name);
                tx.execute(
                    crate::storage::libsql::constant::SQL_DELETE_ENTITY,
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
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn delete_observations(
    conn: &Connection,
    deletions: Vec<crate::model::ObservationDeletion>,
) -> Result<()> {
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let deletions = deletions.clone();
        Box::pin(async move {
            for mut del in deletions {
                del.entity_name = crate::normalize::normalize_key(&del.entity_name);
                for obs in del.observations {
                    tx.execute(
                        crate::storage::libsql::constant::SQL_DELETE_OBSERVATION,
                        libsql::params![del.entity_name.clone(), obs],
                    )
                    .await?;
                }
            }
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn delete_observation_by_id(conn: &Connection, entity_name: &str, id: i64) -> Result<()> {
    let norm_name = crate::normalize::normalize_key(entity_name);
    let affected = conn
        .execute(
            crate::storage::libsql::constant::SQL_DELETE_OBSERVATION_BY_ID,
            libsql::params![id, norm_name],
        )
        .await?;
    if affected == 0 {
        anyhow::bail!(
            "No observation with ID {} belongs to entity '{}'",
            id,
            entity_name
        );
    }
    Ok(())
}

pub async fn update_observation_by_id(
    conn: &Connection,
    entity_name: &str,
    id: i64,
    new_content: &str,
) -> Result<()> {
    let norm_name = crate::normalize::normalize_key(entity_name);
    let affected = conn
        .execute(
            crate::storage::libsql::constant::SQL_UPDATE_OBSERVATION_BY_ID,
            libsql::params![id, new_content, norm_name],
        )
        .await?;
    if affected == 0 {
        anyhow::bail!(
            "No observation with ID {} belongs to entity '{}'",
            id,
            entity_name
        );
    }
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
        crate::storage::libsql::constant::SQL_UPDATE_OBSERVATION,
        libsql::params![norm_name, old_content, new_content],
    )
    .await?;
    Ok(())
}

pub async fn delete_relations(
    conn: &Connection,
    relations: Vec<crate::model::RelationInput>,
) -> Result<()> {
    crate::storage::libsql::tx::immediate_transaction(conn, |tx| {
        let relations = relations.clone();
        Box::pin(async move {
            for mut rel in relations {
                rel.from = crate::normalize::normalize_key(&rel.from);
                rel.to = crate::normalize::normalize_key(&rel.to);
                tx.execute(
                    crate::storage::libsql::constant::SQL_DELETE_RELATION,
                    libsql::params![rel.from, rel.to, rel.relation_type],
                )
                .await?;
            }
            Ok(())
        })
    })
    .await?;
    Ok(())
}

pub async fn read_graph(conn: &Connection) -> Result<crate::model::Graph> {
    let mut entity_names = Vec::new();
    let mut rows = conn
        .query(
            crate::storage::libsql::constant::SQL_SELECT_ALL_ENTITIES,
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        entity_names.push(row.get::<String>(0)?);
    }
    let entities = load_entities_lazy(conn, &entity_names).await?;

    let mut relations = Vec::new();
    let mut rel_rows = conn
        .query(
            crate::storage::libsql::constant::SQL_SELECT_ALL_RELATIONS,
            (),
        )
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
        .query(
            crate::storage::libsql::constant::SQL_SELECT_ALL_ENTITIES,
            (),
        )
        .await?;
    while let Some(row) = rows.next().await? {
        entity_names.push(row.get::<String>(0)?);
    }
    let entities = load_entities_eager(conn, &entity_names).await?;

    let mut relations = Vec::new();
    let mut rel_rows = conn
        .query(
            crate::storage::libsql::constant::SQL_SELECT_ALL_RELATIONS,
            (),
        )
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

/// Entity types that are never included in a scoped export: volatile local state
/// (`session`) and the importer's own global preferences (`preference`,
/// `standard`), which must not be clobbered by an imported bundle.
const SCOPE_EXCLUDED_TYPES: [&str; 3] = ["session", "preference", "standard"];

/// Compute the entity-name set for a scoped export rooted at `roots`:
/// each root, its `part_of` children (transitively, inward), and the
/// `depends_on` targets those cite (one hop, leaf — not followed further).
/// With `rationale`, also pull one hop of `supersedes`/`extends` off the cited
/// leaves. Pure over the loaded [`Graph`] so it is unit-testable without a DB.
pub(crate) fn scope_subgraph(
    graph: &crate::model::Graph,
    roots: &[String],
    rationale: bool,
) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let existing: HashSet<&str> = graph.entities.iter().map(|e| e.name.as_str()).collect();
    let mut keep: HashSet<String> = roots
        .iter()
        .map(|r| crate::normalize::normalize_key(r))
        .filter(|r| existing.contains(r.as_str()))
        .collect();

    // 1. Inward, transitive: pull any `part_of` child of an entity already kept.
    loop {
        let mut added = false;
        for r in &graph.relations {
            if r.relation_type == "part_of" && keep.contains(&r.to) && !keep.contains(&r.from) {
                keep.insert(r.from.clone());
                added = true;
            }
        }
        if !added {
            break;
        }
    }

    // 2. Outward, one hop, leaf: the decisions/pitfalls the kept set cites.
    let leaves: Vec<String> = graph
        .relations
        .iter()
        .filter(|r| r.relation_type == "depends_on" && keep.contains(&r.from))
        .map(|r| r.to.clone())
        .collect();
    keep.extend(leaves.iter().cloned());

    // 3. Optional rationale hop off those leaves.
    if rationale {
        let leafset: HashSet<&String> = leaves.iter().collect();
        for r in &graph.relations {
            if matches!(r.relation_type.as_str(), "supersedes" | "extends")
                && leafset.contains(&r.from)
            {
                keep.insert(r.to.clone());
            }
        }
    }

    // 4. Type guard: never export volatile local or importer-global entities.
    let excluded: HashSet<&str> = graph
        .entities
        .iter()
        .filter(|e| SCOPE_EXCLUDED_TYPES.contains(&e.entity_type.as_str()))
        .map(|e| e.name.as_str())
        .collect();
    keep.retain(|n| !excluded.contains(n.as_str()));

    keep
}

/// Read only the subgraph rooted at `roots` (see [`scope_subgraph`]). Loads the
/// graph eagerly and filters in memory — the graph is small, so this keeps the
/// traversal a pure function rather than recursive SQL. A relation is kept only
/// when both endpoints survive, so the result imports cleanly.
pub async fn read_graph_scoped(
    conn: &Connection,
    roots: &[String],
    rationale: bool,
) -> Result<crate::model::Graph> {
    let full = read_graph_eager(conn).await?;
    let keep = scope_subgraph(&full, roots, rationale);
    Ok(crate::model::Graph {
        entities: full
            .entities
            .into_iter()
            .filter(|e| keep.contains(&e.name))
            .collect(),
        relations: full
            .relations
            .into_iter()
            .filter(|r| keep.contains(&r.from) && keep.contains(&r.to))
            .collect(),
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
        // Primary: FTS5 on observation content (porter stemming, bm25 ranking)
        let fts_hits: Vec<String> = async {
            let fts_fetch_limit = if filters.is_empty() {
                limit.saturating_mul(8).max(limit) as i64
            } else {
                5000
            };
            let mut rows = conn
                .query(
                    crate::storage::libsql::constant::SQL_SEARCH_OBSERVATIONS_FTS,
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
                    crate::storage::libsql::constant::SQL_SEARCH_ENTITIES_LIKE,
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
        let sql = crate::storage::libsql::constant::SQL_SELECT_RELATIONS_IN_TEMPLATE
            .replace("{0}", &placeholders);

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
        let entity_sql = crate::storage::libsql::constant::SQL_SELECT_ENTITIES_IN_TEMPLATE
            .replace("{}", &placeholders);
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
        let entity_sql = crate::storage::libsql::constant::SQL_SELECT_ENTITIES_IN_TEMPLATE
            .replace("{}", &placeholders);
        let params = chunk
            .iter()
            .cloned()
            .map(libsql::Value::from)
            .collect::<Vec<_>>();

        let mut rows = conn.query(&entity_sql, params.clone()).await?;
        while let Some(row) = rows.next().await? {
            entity_types.insert(row.get::<String>(0)?, row.get::<String>(1)?);
        }

        let obs_sql = crate::storage::libsql::constant::SQL_SELECT_OBSERVATIONS_IN_TEMPLATE
            .replace("{}", &placeholders);
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
        let skill_sql = crate::storage::libsql::constant::SQL_SELECT_SKILL_BODIES_IN_TEMPLATE
            .replace("{}", &placeholders);
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
    let mut rows = conn
        .query(crate::storage::libsql::constant::SQL_COUNT_ENTITIES, ())
        .await?;
    let entities_count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    let mut rows = conn
        .query(crate::storage::libsql::constant::SQL_COUNT_RELATIONS, ())
        .await?;
    let relations_count: i64 = if let Some(row) = rows.next().await? {
        row.get(0)?
    } else {
        0
    };

    let mut rows = conn
        .query(crate::storage::libsql::constant::SQL_COUNT_OBSERVATIONS, ())
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
    conn.execute(crate::storage::libsql::constant::SQL_DELETE_ALL_CHUNKS, ())
        .await?;
    conn.execute(crate::storage::libsql::constant::SQL_DELETE_ALL_TOPICS, ())
        .await?;
    conn.execute(
        crate::storage::libsql::constant::SQL_DELETE_ALL_RELATIONS,
        (),
    )
    .await?;
    conn.execute(
        crate::storage::libsql::constant::SQL_DELETE_ALL_OBSERVATIONS,
        (),
    )
    .await?;
    conn.execute(
        crate::storage::libsql::constant::SQL_DELETE_ALL_ENTITIES,
        (),
    )
    .await?;
    Ok(())
}

pub async fn truth_upsert(conn: &Connection, entity: &str, key: &str, value: &str) -> Result<()> {
    let norm_entity = crate::normalize::normalize_key(entity);
    conn.execute(
        crate::storage::libsql::constant::SQL_UPSERT_TRUTH,
        libsql::params![norm_entity, key, value],
    )
    .await?;
    Ok(())
}

pub async fn truth_delete(conn: &Connection, entity: &str, key: &str) -> Result<()> {
    let norm_entity = crate::normalize::normalize_key(entity);
    conn.execute(
        crate::storage::libsql::constant::SQL_DELETE_TRUTH,
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
        let sql = crate::storage::libsql::constant::SQL_SELECT_TRUTHS_FOR_ENTITIES
            .replace("{}", &placeholders);
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
        crate::storage::libsql::constant::SQL_UPSERT_SKILL,
        libsql::params![norm_entity, body, source, version],
    )
    .await?;
    Ok(())
}

pub async fn skill_body(conn: &Connection, entity: &str) -> Result<Option<String>> {
    let norm_entity = crate::normalize::normalize_key(entity);
    let mut rows = conn
        .query(
            crate::storage::libsql::constant::SQL_SELECT_SKILL_BODY,
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
    let mut rows = conn
        .query(crate::storage::libsql::constant::SQL_LIST_SKILLS, ())
        .await?;
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

    fn ent(name: &str, ty: &str) -> crate::model::EntityOutput {
        crate::model::EntityOutput {
            name: name.to_string(),
            entity_type: ty.to_string(),
            observations: Vec::new(),
            truths: std::collections::BTreeMap::new(),
            observation_count: 0,
            body: None,
            observations_detailed: None,
        }
    }

    fn rel(from: &str, to: &str, ty: &str) -> crate::model::RelationInput {
        crate::model::RelationInput {
            from: from.to_string(),
            to: to.to_string(),
            relation_type: ty.to_string(),
        }
    }

    fn scope_fixture() -> crate::model::Graph {
        crate::model::Graph {
            entities: vec![
                ent("proj", "project"),
                ent("proj:a", "task"),
                ent("proj:a:task-1", "task"),
                ent("proj:a:task-2", "task"),
                ent("proj:b", "task"),
                ent("proj:b:task-1", "task"),
                ent("proj:decision:x", "concept"),
                ent("proj:decision:root", "concept"),
                ent("proj:pitfall:shared", "concept"),
                ent("UserPreferences", "preference"),
            ],
            relations: vec![
                rel("proj:a", "proj", "part_of"),
                rel("proj:a:task-1", "proj:a", "part_of"),
                rel("proj:a:task-2", "proj:a", "part_of"),
                rel("proj:b", "proj", "part_of"),
                rel("proj:b:task-1", "proj:b", "part_of"),
                rel("proj:a:task-2", "proj:decision:x", "depends_on"),
                rel("proj:decision:x", "proj:decision:root", "extends"),
                rel("proj:a:task-1", "proj:pitfall:shared", "depends_on"),
                rel("proj:b:task-1", "proj:pitfall:shared", "depends_on"),
            ],
        }
    }

    #[test]
    fn scope_pulls_epic_and_part_of_children() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:b".to_string()], false);
        assert!(keep.contains("proj:b"));
        assert!(keep.contains("proj:b:task-1"));
        assert!(!keep.contains("proj:a"));
        assert!(!keep.contains("proj"));
    }

    #[test]
    fn scope_includes_cited_leaves_but_stops_there() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:a".to_string()], false);
        assert!(keep.contains("proj:a:task-2"));
        assert!(keep.contains("proj:decision:x"));
        assert!(!keep.contains("proj:decision:root"));
    }

    #[test]
    fn scope_rationale_follows_one_extends_hop() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:a".to_string()], true);
        assert!(keep.contains("proj:decision:x"));
        assert!(keep.contains("proj:decision:root"));
    }

    #[test]
    fn scope_shared_pitfall_does_not_drag_sibling() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:a".to_string()], false);
        assert!(keep.contains("proj:pitfall:shared"));
        assert!(!keep.contains("proj:b"));
        assert!(!keep.contains("proj:b:task-1"));
    }

    #[test]
    fn scope_unions_multiple_roots() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:a".to_string(), "proj:b".to_string()], false);
        assert!(keep.contains("proj:a:task-1"));
        assert!(keep.contains("proj:b:task-1"));
        assert!(!keep.contains("proj"));
    }

    #[test]
    fn scope_type_guard_excludes_preferences() {
        let mut g = scope_fixture();
        g.relations
            .push(rel("proj:a:task-1", "UserPreferences", "depends_on"));
        let keep = scope_subgraph(&g, &["proj:a".to_string()], false);
        assert!(!keep.contains("UserPreferences"));
    }

    #[test]
    fn scope_ignores_unknown_roots() {
        let g = scope_fixture();
        let keep = scope_subgraph(&g, &["proj:does-not-exist".to_string()], false);
        assert!(keep.is_empty());
    }

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

        let mut rows = conn
            .query(
                "SELECT name FROM sqlite_master WHERE name='asobi_obs_fts'",
                (),
            )
            .await
            .unwrap();
        let row = rows.next().await.unwrap();
        assert!(row.is_some(), "asobi_obs_fts table missing");
    }

    #[tokio::test]
    async fn test_observation_fts_rebuilds_after_legacy_id_migration() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("legacy.db");
        unsafe {
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        let legacy_db = libsql::Builder::new_local(db_path.to_str().unwrap())
            .build()
            .await
            .unwrap();
        let legacy_conn = legacy_db.connect().unwrap();
        legacy_conn
            .execute(
                "CREATE TABLE asobi_entities (name TEXT PRIMARY KEY, entity_type TEXT NOT NULL)",
                (),
            )
            .await
            .unwrap();
        legacy_conn
            .execute(
                "CREATE TABLE asobi_observations (id TEXT PRIMARY KEY, entity_name TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
                (),
            )
            .await
            .unwrap();
        legacy_conn
            .execute(
                "INSERT INTO asobi_entities VALUES ('legacy', 'project')",
                (),
            )
            .await
            .unwrap();
        legacy_conn
            .execute(
                "INSERT INTO asobi_observations (id, entity_name, content) VALUES ('old', 'legacy', 'to be deleted'), ('survivor', 'legacy', 'migration survivor')",
                (),
            )
            .await
            .unwrap();
        legacy_conn
            .execute("DELETE FROM asobi_observations WHERE id = 'old'", ())
            .await
            .unwrap();
        legacy_conn
            .execute("PRAGMA user_version = 0", ())
            .await
            .unwrap();
        drop(legacy_conn);
        drop(legacy_db);

        let (_db, conn) = init_db().await.unwrap();
        let graph = search_nodes(&conn, "migration survivor").await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "legacy");
    }

    // A pre-v2 database has a 384-wide `chunks.embedding` column (all-MiniLM-L6-v2).
    // Opening it under the v2 schema (gte-base-en-v1.5, 768-dim) must drop and
    // recreate the table at the new dimension: old chunks are gone, the column is
    // 768-wide, and user_version advances to SCHEMA_VERSION.
    #[tokio::test]
    async fn test_chunks_recreated_at_new_dim_on_v2_migration() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("pre_v2.db");
        unsafe {
            std::env::set_var(ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        let legacy_db = libsql::Builder::new_local(db_path.to_str().unwrap())
            .build()
            .await
            .unwrap();
        let legacy_conn = legacy_db.connect().unwrap();
        legacy_conn
            .execute(
                "CREATE TABLE chunks (
                    id        TEXT PRIMARY KEY,
                    topic_id  TEXT NOT NULL,
                    chunk_idx INTEGER NOT NULL,
                    text      TEXT NOT NULL,
                    source    TEXT NOT NULL,
                    embedding F32_BLOB(384) NOT NULL
                )",
                (),
            )
            .await
            .unwrap();
        legacy_conn
            .execute(
                "INSERT INTO chunks (id, topic_id, chunk_idx, text, source, embedding) \
                 VALUES ('stale', 'topic', 0, 'old', 'old.md', vector32(?1))",
                libsql::params![serde_json::to_string(&vec![0.0f32; 384]).unwrap()],
            )
            .await
            .unwrap();
        legacy_conn
            .execute("PRAGMA user_version = 1", ())
            .await
            .unwrap();
        drop(legacy_conn);
        drop(legacy_db);

        let (_db, conn) = init_db().await.unwrap();

        // Old 384-dim chunk was dropped, not carried over.
        let mut rows = conn.query("SELECT COUNT(*) FROM chunks", ()).await.unwrap();
        let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(count, 0, "stale pre-v2 chunk should be dropped");

        // Embedding column is now 768-wide.
        let mut info = conn.query("PRAGMA table_info(chunks)", ()).await.unwrap();
        let mut embedding_type = None;
        while let Some(row) = info.next().await.unwrap() {
            let name: String = row.get(1).unwrap();
            if name == "embedding" {
                embedding_type = Some(row.get::<String>(2).unwrap());
            }
        }
        assert_eq!(
            embedding_type.as_deref(),
            Some("F32_BLOB(768)"),
            "chunks.embedding should be recreated at dim 768"
        );

        // Schema version advanced.
        let mut ver = conn.query("PRAGMA user_version", ()).await.unwrap();
        let version: i64 = ver.next().await.unwrap().unwrap().get(0).unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        // A fresh 768-dim chunk inserts cleanly against the new column.
        conn.execute(
            "INSERT INTO chunks (id, topic_id, chunk_idx, text, source, embedding) \
             VALUES ('fresh', 'topic', 0, 'new', 'new.md', vector32(?1))",
            libsql::params![serde_json::to_string(&vec![0.0f32; 768]).unwrap()],
        )
        .await
        .unwrap();
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
            "INSERT INTO topics (id, title, file_path, body) VALUES ('rust-pin', 'Rust Pinning', '.asobi/topics/rust-pinning.md', 'pinning is a mechanism...')",
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

        conn.execute(
            "INSERT INTO topics (id, title, file_path, body, created_at) \
             VALUES ('t1', 'Old Title', '/old', 'old body', '2000-01-01 00:00:00')",
            (),
        )
        .await
        .unwrap();

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

        assert_eq!(title, "New Title");
        assert_eq!(body, "new body");
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

        // FTS5 porter stemming: "running" (query) should match "running" (indexed) —
        // and this is also where libsql regains real porter stemming over turso's
        // token-matching approximation, so "run" would stem-match "running" too.
        let graph = search_nodes(&conn, "running").await.unwrap();
        assert_eq!(graph.entities.len(), 1);
        assert_eq!(graph.entities[0].name, "async-patterns");

        let stemmed = search_nodes(&conn, "run").await.unwrap();
        assert_eq!(stemmed.entities.len(), 1);
        assert_eq!(stemmed.entities[0].name, "async-patterns");
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

        // Invalid FTS syntax — must not panic, falls back to LIKE gracefully.
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

        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (0, 0, 0));

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

        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (2, 1, 3));

        reset(&conn).await.unwrap();

        let s = stats(&conn).await.unwrap();
        assert_eq!(s, (0, 0, 0));

        conn.execute(
            "INSERT INTO topics (id, title, file_path) VALUES ('topic', 'Topic', 'topic.md')",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "INSERT INTO chunks (id, topic_id, chunk_idx, text, source, embedding) VALUES ('chunk', 'topic', 0, 'text', 'source', vector32(?1))",
            libsql::params![serde_json::to_string(&vec![0.0f32; 768]).unwrap()],
        )
        .await
        .unwrap();
        reset(&conn).await.unwrap();
        let mut rows = conn
            .query(
                "SELECT COUNT(*) FROM topics UNION ALL SELECT COUNT(*) FROM chunks",
                (),
            )
            .await
            .unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn test_create_entities_does_not_reseed_existing_observations() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        let entity = crate::model::EntityInput {
            name: "project".to_string(),
            entity_type: "task".to_string(),
            observations: vec!["initial".to_string()],
        };
        create_entities(&conn, vec![entity.clone()]).await.unwrap();
        create_entities(&conn, vec![entity]).await.unwrap();

        let graph = open_nodes(&conn, vec!["project".to_string()])
            .await
            .unwrap();
        assert_eq!(graph.entities[0].observations, vec!["initial"]);
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
    async fn test_observation_id_mutations_are_scoped_to_entity() {
        let dir = tempdir().unwrap();
        unsafe {
            std::env::set_var(
                ENV_DATABASE_URL,
                dir.path().join("test.db").to_str().unwrap(),
            );
        }
        let (_db, conn) = init_db().await.unwrap();

        seed_entity(&conn, "alice", "person", &["alice observation"]).await;
        seed_entity(&conn, "bob", "person", &["bob observation"]).await;

        let delete_err = delete_observation_by_id(&conn, "bob", 1).await.unwrap_err();
        assert!(delete_err.to_string().contains("belongs to entity 'bob'"));

        let update_err = update_observation_by_id(&conn, "bob", 1, "mutated")
            .await
            .unwrap_err();
        assert!(update_err.to_string().contains("belongs to entity 'bob'"));

        let graph = open_nodes(&conn, vec!["alice".to_string(), "bob".to_string()])
            .await
            .unwrap();
        assert_eq!(graph.entities[0].observations, vec!["alice observation"]);
        assert_eq!(graph.entities[1].observations, vec!["bob observation"]);
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

        let graph_read = read_graph(&conn).await.unwrap();
        let entity_read = &graph_read.entities[0];
        assert!(entity_read.observations.is_empty());
        assert_eq!(entity_read.observation_count, 2);
        assert_eq!(entity_read.truths.len(), 1);
        assert_eq!(entity_read.truths.get("k1").unwrap(), "v1");

        let graph_search = search_nodes(&conn, "test").await.unwrap();
        let entity_search = &graph_search.entities[0];
        assert!(entity_search.observations.is_empty());
        assert_eq!(entity_search.observation_count, 2);
        assert_eq!(entity_search.truths.len(), 1);
        assert_eq!(entity_search.truths.get("k1").unwrap(), "v1");

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

        skill_upsert(
            &conn,
            "skill:test-skill",
            "body content 1",
            "source-1",
            "1.0.0",
        )
        .await
        .unwrap();

        let graph = open_nodes(&conn, vec!["skill:test-skill".to_string()])
            .await
            .unwrap();
        let entity = &graph.entities[0];
        assert_eq!(entity.body.as_deref(), Some("body content 1"));

        let graph_read = read_graph(&conn).await.unwrap();
        assert!(graph_read.entities[0].body.is_none());

        let graph_search = search_nodes(&conn, "skill").await.unwrap();
        assert!(graph_search.entities[0].body.is_none());

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

        let skills = list_skills(&conn).await.unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].entity_name, "skill:test-skill");
        assert_eq!(skills[0].description, "my test skill");
        assert_eq!(skills[0].version, "1.0.1");
        assert_eq!(skills[0].source, "source-1");

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
