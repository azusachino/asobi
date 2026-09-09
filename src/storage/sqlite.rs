use crate::api::v2::{
    ApiError, ApiResult, BackendCapabilities, BackendHealth, BackupReceipt, BackupRequest,
    BackupStore, GraphStore, MaintenanceStore, OpenNodes, PurgeCandidate, PurgeReport,
    PurgeRequest, SearchQuery, SearchStore, Stats, StorageLocation, TaskStore,
};
use crate::model::{
    EntityInput, EntityOutput, Graph, ObservationDeletion, ObservationInput, RelationInput,
};
use rusqlite::{
    Connection, OptionalExtension, ToSql, Transaction, TransactionBehavior, params,
    params_from_iter,
};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA_VERSION: i64 = 8;
const DEFAULT_DATABASE_FILENAME: &str = "asobi.db";
const DEFAULT_BUSY_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_OBSERVATION_LIMIT: usize = 200;
const PURGEABLE_ENTITY_TYPES: &[&str] = &["session", "task"];
const PURGEABLE_STATUSES: &[&str] = &["DONE", "CLOSED", "ABANDONED"];

fn backend_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::Backend(error.to_string())
}

fn normalize(value: &str) -> String {
    crate::normalize::normalize_key(value)
}

fn collect_purge_candidates(
    conn: &Connection,
    request: &PurgeRequest,
) -> rusqlite::Result<Vec<PurgeCandidate>> {
    let type_placeholders = std::iter::repeat_n("?", PURGEABLE_ENTITY_TYPES.len())
        .collect::<Vec<_>>()
        .join(",");
    let status_placeholders = std::iter::repeat_n("?", PURGEABLE_STATUSES.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        SELECT name, entity_type, status, last_activity, observations, relations
        FROM (
            SELECT e.name, e.entity_type,
                COALESCE(
                    (SELECT value FROM asobi_truths
                     WHERE entity_name = e.name AND key = 'status'),
                    ''
                ) AS status,
                MAX(
                    e.created_at,
                    COALESCE(
                        (SELECT MAX(created_at) FROM asobi_observations
                         WHERE entity_name = e.name),
                        e.created_at
                    ),
                    COALESCE(
                        (SELECT MAX(updated_at) FROM asobi_truths
                         WHERE entity_name = e.name),
                        e.created_at
                    )
                ) AS last_activity,
                (SELECT COUNT(*) FROM asobi_observations
                 WHERE entity_name = e.name) AS observations,
                (SELECT COUNT(*) FROM asobi_relations
                 WHERE from_entity = e.name OR to_entity = e.name) AS relations
            FROM asobi_entities e
            WHERE e.entity_type IN ({type_placeholders})
        ) candidates
        WHERE status IN ({status_placeholders})
          AND last_activity < datetime('now', ?)
        ORDER BY last_activity, name
        "#
    );
    let cutoff = format!("-{} days", request.older_than_days);
    let mut values: Vec<&dyn ToSql> = Vec::new();
    values.extend(PURGEABLE_ENTITY_TYPES.iter().map(|v| v as &dyn ToSql));
    values.extend(PURGEABLE_STATUSES.iter().map(|v| v as &dyn ToSql));
    values.push(&cutoff);

    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map(params_from_iter(values), |row| {
        Ok(PurgeCandidate {
            name: row.get(0)?,
            entity_type: row.get(1)?,
            status: row.get(2)?,
            last_activity: row.get(3)?,
            observations: row.get::<_, i64>(4)? as usize,
            relations: row.get::<_, i64>(5)? as usize,
        })
    })?
    .collect()
}

/// Entities matching `term`, across every path Asobi indexes: observation
/// text, truth values, and the entity's own name or type.
///
/// Truth values were unsearchable before 0.7, which mattered because the
/// convention is to store a pitfall's human-readable warning in a `title`
/// truth -- so the one sentence explaining a dead end was the one thing recall
/// could not reach.
/// How many candidates each path contributes to the fusion, before the caller
/// truncates to its own limit.
///
/// Fixed rather than derived from `--limit`, so ranking does not shift when the
/// caller asks for more: capping each path at the requested limit meant a wider
/// request pulled in weaker candidates that diluted the fused order, and the
/// same entity moved position depending on how many results were asked for.
const CANDIDATE_POOL: i64 = 100;

fn matching_names(conn: &Connection, term: &str, limit: i64) -> rusqlite::Result<Vec<String>> {
    // A negative limit means "unbounded" (a filtered search post-filters), and
    // the pool has to be at least as generous as the caller's request.
    let pool = if limit < 0 {
        -1
    } else {
        limit.max(CANDIDATE_POOL)
    };
    let ranked = |sql: &str, params: &[&dyn ToSql]| -> rusqlite::Result<Vec<String>> {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params_from_iter(params.iter().copied()), |r| {
            r.get::<_, String>(0)
        });
        // A malformed FTS5 query is a user error, not a failure: the other
        // paths still answer, and the LIKE fallback usually does.
        Ok(match rows {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => Vec::new(),
        })
    };

    let like = format!("%{term}%");
    let paths = [
        ranked(
            "SELECT DISTINCT o.entity_name FROM asobi_obs_fts
             JOIN asobi_observations o ON asobi_obs_fts.rowid = o.rowid
             WHERE asobi_obs_fts MATCH ? ORDER BY bm25(asobi_obs_fts) LIMIT ?",
            &[&term, &pool],
        )?,
        ranked(
            "SELECT DISTINCT t.entity_name FROM asobi_truth_fts
             JOIN asobi_truths t ON asobi_truth_fts.rowid = t.rowid
             WHERE asobi_truth_fts MATCH ? ORDER BY bm25(asobi_truth_fts) LIMIT ?",
            &[&term, &pool],
        )?,
        ranked(
            "SELECT name FROM asobi_entities
             WHERE name LIKE ? OR entity_type LIKE ? ORDER BY name LIMIT ?",
            &[&like, &like, &pool],
        )?,
    ];

    // Reciprocal rank fusion across the three paths.
    //
    // Concatenating them meant every observation match outranked every truth
    // match, so an entity found only by its `title` landed last however good
    // the match was -- and the ordering shifted with `--limit`, since each path
    // was capped before the merge. Fusing by rank instead lets a strong match
    // in one path compete with a strong match in another, and rewards an entity
    // that several paths agree on. The constant 60 is the conventional RRF
    // damping value: large enough that the top few ranks are not runaway
    // favourites, small enough that rank still dominates.
    const RRF_DAMPING: f64 = 60.0;
    let mut scored: BTreeMap<String, f64> = BTreeMap::new();
    for path in &paths {
        for (rank, name) in path.iter().enumerate() {
            *scored.entry(name.clone()).or_insert(0.0) += 1.0 / (RRF_DAMPING + rank as f64 + 1.0);
        }
    }
    let mut fused: Vec<(String, f64)> = scored.into_iter().collect();
    // Name as the tiebreak, so equal scores order deterministically.
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(fused.into_iter().map(|(name, _)| name).collect())
}

/// Rewrite a multi-word query as `a OR b OR c`, or `None` when there is nothing
/// to widen -- a single term, or one already carrying FTS5 operators, where
/// rewriting would either change nothing or corrupt the caller's intent.
fn widen_query(term: &str) -> Option<String> {
    let words: Vec<&str> = term.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    if words
        .iter()
        .any(|w| matches!(*w, "AND" | "OR" | "NOT") || w.contains('"'))
    {
        return None;
    }
    Some(words.join(" OR "))
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
    /// Whether this process has already run the retention sweep.
    retention_swept: std::sync::atomic::AtomicBool,
}

/// How long a finished session or task survives before the automatic sweep
/// removes it.
///
/// Operational state is relevant for hours, occasionally days: a task that has
/// been `DONE` for a week is not context, it is archaeology. The previous
/// design left this to a manual `purge` that was correct in every respect
/// except that it never ran -- six weeks of daily use left a graph that was 96%
/// finished work. A default that has to be invoked is a default that does not
/// happen.
pub const DEFAULT_RETENTION_DAYS: u32 = 7;

impl SqliteStore {
    pub fn open_default() -> crate::Result<Self> {
        let paths = crate::paths::AsobiPaths::resolve();
        let path = std::env::var(crate::paths::ENV_DATABASE_URL)
            .map(PathBuf::from)
            .unwrap_or_else(|_| paths.data_dir.join(DEFAULT_DATABASE_FILENAME));
        Self::open_at(&path)
    }

    pub fn open_at(path: &Path) -> crate::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(
            std::env::var("ASOBI_BUSY_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_BUSY_TIMEOUT_MS),
        ))?;
        let previous_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if previous_version == 0 {
            // `auto_vacuum` only takes effect before the database file's
            // header is first written, which `journal_mode=WAL` below does
            // as a side effect -- so this must run first, or the pragma
            // silently no-ops and the database is stuck at auto_vacuum=NONE
            // forever. A pre-existing database is switched over in
            // `upgrade_to_v5` instead, which pays for it with the one-time
            // VACUUM that changing this pragma later requires.
            conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
        }
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        Self::init_schema(&conn, previous_version)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: path.to_path_buf(),
            retention_swept: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn init_schema(conn: &Connection, previous_version: i64) -> rusqlite::Result<()> {
        if previous_version > 0 && previous_version < 5 {
            Self::upgrade_to_v5(conn)?;
        }
        if previous_version > 0 && previous_version < 6 {
            Self::upgrade_to_v6(conn)?;
        }
        if previous_version > 0 && previous_version < 7 {
            Self::upgrade_to_v7(conn)?;
        }
        if previous_version > 0 && previous_version < 8 {
            // The truth index is created by the schema batch below; an existing
            // database needs it populated from rows that predate the triggers.
            conn.execute_batch("DROP TABLE IF EXISTS asobi_truth_fts;")?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS asobi_entities (
                name TEXT PRIMARY KEY,
                entity_type TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE IF NOT EXISTS asobi_observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_name TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE INDEX IF NOT EXISTS idx_observations_entity ON asobi_observations(entity_name);
            CREATE TABLE IF NOT EXISTS asobi_relations (
                from_entity TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
                to_entity TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
                relation_type TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (from_entity, to_entity, relation_type)
            );
            CREATE INDEX IF NOT EXISTS idx_relations_to ON asobi_relations(to_entity, from_entity, relation_type);
            CREATE TABLE IF NOT EXISTS asobi_truths (
                entity_name TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                PRIMARY KEY (entity_name, key)
            );
            CREATE INDEX IF NOT EXISTS idx_truths_lookup ON asobi_truths(key, value, entity_name);
            CREATE VIRTUAL TABLE IF NOT EXISTS asobi_truth_fts USING fts5(
                value, content='asobi_truths', content_rowid='rowid',
                tokenize='porter unicode61'
            );
            CREATE TRIGGER IF NOT EXISTS asobi_truth_ai AFTER INSERT ON asobi_truths BEGIN
                INSERT INTO asobi_truth_fts(rowid, value) VALUES (new.rowid, new.value);
            END;
            CREATE TRIGGER IF NOT EXISTS asobi_truth_ad AFTER DELETE ON asobi_truths BEGIN
                INSERT INTO asobi_truth_fts(asobi_truth_fts, rowid, value) VALUES ('delete', old.rowid, old.value);
            END;
            CREATE TRIGGER IF NOT EXISTS asobi_truth_au AFTER UPDATE ON asobi_truths BEGIN
                INSERT INTO asobi_truth_fts(asobi_truth_fts, rowid, value) VALUES ('delete', old.rowid, old.value);
                INSERT INTO asobi_truth_fts(rowid, value) VALUES (new.rowid, new.value);
            END;
            CREATE VIRTUAL TABLE IF NOT EXISTS asobi_obs_fts USING fts5(
                content, content='asobi_observations', content_rowid='rowid',
                tokenize='porter unicode61'
            );
            CREATE TRIGGER IF NOT EXISTS asobi_obs_ai AFTER INSERT ON asobi_observations BEGIN
                INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS asobi_obs_ad AFTER DELETE ON asobi_observations BEGIN
                INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS asobi_obs_au AFTER UPDATE ON asobi_observations BEGIN
                INSERT INTO asobi_obs_fts(asobi_obs_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
                INSERT INTO asobi_obs_fts(rowid, content) VALUES (new.rowid, new.content);
            END;
            PRAGMA user_version = 8;",
        )?;
        let count: i64 =
            conn.query_row("SELECT count(*) FROM asobi_observations", [], |r| r.get(0))?;
        if count > 0 && previous_version < SCHEMA_VERSION {
            let _ = conn.execute(
                "INSERT INTO asobi_obs_fts(asobi_obs_fts) VALUES ('rebuild')",
                [],
            );
        }
        let truths: i64 = conn.query_row("SELECT count(*) FROM asobi_truths", [], |r| r.get(0))?;
        if truths > 0 && previous_version < SCHEMA_VERSION {
            let _ = conn.execute(
                "INSERT INTO asobi_truth_fts(asobi_truth_fts) VALUES ('rebuild')",
                [],
            );
        }
        Ok(())
    }

    /// Every asobi generation before the 0.6 rusqlite rewrite (`mcp_*`
    /// tables from the original MCP-memory schema, then `chunks`/`topics`
    /// from the libSQL/Turso hybrid vector search era) left its tables in
    /// place when superseded rather than dropping them, since the old
    /// migration path only ever added the next generation's tables. On a
    /// database that has been open continuously since before this fix,
    /// those tables are pure dead weight — observed as high as 96% of
    /// on-disk pages on a long-lived real database, and permanent, since
    /// SQLite never shrinks a file after a DELETE without an explicit
    /// VACUUM (see ADR 0003, "Compaction and physical storage").
    ///
    /// `chunks` carries a `libsql_vector_idx(...)` expression index that
    /// only the libSQL fork's SQLite build can resolve; this rusqlite build
    /// cannot, so the table (and with it, that index) must be dropped
    /// before VACUUM rewrites the file, or VACUUM itself fails trying to
    /// rebuild it. Dropping a table also drops its own indexes and
    /// triggers, so no companion DROP INDEX/TRIGGER statements are needed.
    fn upgrade_to_v5(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "DROP TABLE IF EXISTS mcp_obs_fts;
             DROP TABLE IF EXISTS mcp_skills;
             DROP TABLE IF EXISTS mcp_truths;
             DROP TABLE IF EXISTS mcp_relations;
             DROP TABLE IF EXISTS mcp_observations;
             DROP TABLE IF EXISTS mcp_entities;
             DROP TABLE IF EXISTS chunks;
             DROP TABLE IF EXISTS idx_chunks_vector_shadow;
             DROP TABLE IF EXISTS libsql_vector_meta_shadow;
             DROP TABLE IF EXISTS topics_fts;
             DROP TABLE IF EXISTS topics;
             DROP TABLE IF EXISTS sessions;",
        )?;
        conn.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// 0.7 moved skills out of the graph and onto the filesystem, where the
    /// Agent Skills ecosystem already expects them and where `rg` can reach
    /// them. The graph copy was never the one agents read: it held a body, a
    /// source and a version, and in practice accumulated nothing else — no
    /// observations, and only a `description` truth.
    ///
    /// So drop the table and the entities it hung off. The skill entities go
    /// too rather than being left as empty husks, since a `skill`-typed entity
    /// with no body is not a thing any reader wants back; cascades take their
    /// truths, observations and relations with them. Whatever was installed is
    /// already on disk under the skills directory, and `skills sync` rewrites
    /// that from `asobi.toml` regardless, so nothing here is the only copy.
    /// Runs before `init_schema`'s `CREATE TABLE IF NOT EXISTS` batch, so on a
    /// database old enough to predate the current generation entirely there is
    /// nothing here to clean up yet — hence the existence check rather than an
    /// unconditional `DELETE`.
    fn upgrade_to_v6(conn: &Connection) -> rusqlite::Result<()> {
        let has_entities: bool = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='asobi_entities'",
            [],
            |r| r.get::<_, i64>(0).map(|n| n > 0),
        )?;
        if has_entities {
            conn.execute("DELETE FROM asobi_entities WHERE entity_type = 'skill'", [])?;
        }
        conn.execute_batch("DROP TABLE IF EXISTS asobi_skills;")?;
        conn.execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    /// 0.7 dropped superseded truth versions. The table recorded every value a
    /// truth had ever held, unbounded and cascading only on entity delete --
    /// the one store in Asobi with no limit of its own, growing fastest on
    /// whatever was written most often. On a real six-week-old graph that was
    /// 616 rows, 496 of them sessions whose `next` had been rewritten 139 times.
    ///
    /// It had no reader. `asobi history` appeared in no workflow, and where a
    /// trail genuinely mattered the observations already carried it in better
    /// form: a task's history held one row saying `status=DISPATCHED`, next to
    /// an observation saying "dispatched to codex". A bi-temporal store answers
    /// questions about how state changed over time; nothing here asked one.
    fn upgrade_to_v7(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch("DROP TABLE IF EXISTS asobi_truth_history;")?;
        conn.execute_batch("PRAGMA incremental_vacuum;")?;
        Ok(())
    }

    /// Drop finished operational entities once per process, before the first
    /// write.
    ///
    /// On a write rather than at open, so a pure read never mutates the graph --
    /// `asobi show` must not delete anything. Once per process rather than per
    /// call, since a single command should not pay for the sweep repeatedly.
    /// Failures are ignored: retention is hygiene, and a command must not fail
    /// because housekeeping did.
    fn sweep_expired_once(&self) {
        use std::sync::atomic::Ordering;
        if self.retention_swept.swap(true, Ordering::SeqCst) {
            return;
        }
        // Resolved the same way as `observation_limit`, the other bound in
        // this tool: environment first, then `asobi.toml`, then the default.
        let days = std::env::var("ASOBI_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or_else(|| {
                crate::paths::AsobiPaths::resolve()
                    .retention_days
                    .unwrap_or(DEFAULT_RETENTION_DAYS)
            });
        if days == 0 {
            return;
        }
        let _ = self.purge(PurgeRequest {
            older_than_days: days,
            apply: true,
        });
    }

    fn write<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> ApiResult<T> {
        self.sweep_expired_once();
        let mut conn = self
            .conn
            .lock()
            .map_err(|_| ApiError::Unavailable("database mutex poisoned".into()))?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(backend_error)?;
        match operation(&tx) {
            Ok(value) => {
                tx.commit().map_err(backend_error)?;
                Ok(value)
            }
            Err(error) => Err(backend_error(error)),
        }
    }

    fn read<T>(&self, operation: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> ApiResult<T> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| ApiError::Unavailable("database mutex poisoned".into()))?;
        operation(&conn).map_err(backend_error)
    }

    /// `observation_limit` of 0 means every observation; anything else returns
    /// the most recent N. `observationCount` stays the true total either way,
    /// so a caller can always tell what it is not being shown.
    fn graph(
        &self,
        names: Option<&[String]>,
        expand: &[String],
        include_content: bool,
        observation_limit: usize,
    ) -> ApiResult<Graph> {
        self.read(|conn| {
            graph_from_connection(conn, names, expand, include_content, observation_limit)
        })
    }
}

fn graph_from_connection(
    conn: &Connection,
    names: Option<&[String]>,
    expand: &[String],
    include_content: bool,
    observation_limit: usize,
) -> rusqlite::Result<Graph> {
    let mut selected = names.map(|values| values.iter().map(|v| normalize(v)).collect::<Vec<_>>());
    if let Some(values) = selected.as_mut()
        && !expand.is_empty()
        && !values.is_empty()
    {
        let placeholders = (0..values.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let mut stmt = conn.prepare(&format!("SELECT from_entity, to_entity, relation_type FROM asobi_relations WHERE from_entity IN ({placeholders}) OR to_entity IN ({placeholders})"))?;
        let mut params_vec: Vec<&dyn rusqlite::ToSql> = Vec::new();
        for value in values.iter() {
            params_vec.push(value);
        }
        for value in values.iter() {
            params_vec.push(value);
        }
        let rows = stmt.query_map(rusqlite::params_from_iter(params_vec), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (from, to, kind) = row?;
            if expand.iter().any(|wanted| wanted == &kind) {
                if !values.contains(&from) {
                    values.push(from);
                }
                if !values.contains(&to) {
                    values.push(to);
                }
            }
        }
    }

    let entity_sql = match selected.as_ref() {
        Some(values) if values.is_empty() => {
            "SELECT name, entity_type FROM asobi_entities WHERE 0".to_string()
        }
        Some(values) => format!(
            "SELECT name, entity_type FROM asobi_entities WHERE name IN ({})",
            (0..values.len()).map(|_| "?").collect::<Vec<_>>().join(",")
        ),
        None => "SELECT name, entity_type FROM asobi_entities ORDER BY name".to_string(),
    };
    let values = selected.unwrap_or_default();
    let entity_rows: Vec<(String, String)> = if entity_sql.contains("IN (") {
        let mut stmt = conn.prepare(&entity_sql)?;
        let mut rows = stmt
            .query_map(rusqlite::params_from_iter(values.iter()), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // `IN (...)` returns rows in whatever order SQLite likes, so restore
        // the caller's. That order is meaningful: for `search` it is the fused
        // relevance ranking, and for `show` it is the order the names were
        // asked for. Sorting by name here -- which is what this did -- silently
        // discarded every ranking the search had just computed.
        let position: std::collections::HashMap<&str, usize> = values
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect();
        rows.sort_by_key(|(name, _)| position.get(name.as_str()).copied().unwrap_or(usize::MAX));
        rows
    } else {
        let mut stmt = conn.prepare(&entity_sql)?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut entities = Vec::new();
    let mut obs_stmt = if include_content {
        // Newest-first inside the limit, then re-ordered oldest-first so a
        // truncated trail still reads in the direction it was written.
        Some(conn.prepare(
            "SELECT id, content FROM (
                 SELECT id, content FROM asobi_observations
                 WHERE entity_name = ? ORDER BY id DESC LIMIT ?
             ) ORDER BY id",
        )?)
    } else {
        None
    };
    // Prepared unconditionally: the true total is what tells a caller how much
    // a limited read left behind.
    let mut obs_count_stmt =
        conn.prepare("SELECT COUNT(*) FROM asobi_observations WHERE entity_name = ?")?;
    let mut truth_stmt =
        conn.prepare("SELECT key, value FROM asobi_truths WHERE entity_name = ? ORDER BY key")?;
    for (name, entity_type) in entity_rows {
        let observation_count = obs_count_stmt.query_row([&name], |r| r.get::<_, i64>(0))? as usize;
        let (observations, observations_detailed) = if include_content {
            let cap = if observation_limit == 0 {
                i64::MAX
            } else {
                observation_limit as i64
            };
            let mut observations = Vec::new();
            let mut detailed = Vec::new();
            for obs in obs_stmt
                .as_mut()
                .expect("content query must be prepared")
                .query_map(params![&name, cap], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
            {
                let (id, content) = obs?;
                observations.push(content.clone());
                detailed.push(crate::model::DetailedObservation { id, content });
            }
            (observations, Some(detailed))
        } else {
            (Vec::new(), None)
        };
        let mut truths = BTreeMap::new();
        for truth in truth_stmt.query_map([&name], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (key, value) = truth?;
            truths.insert(key, value);
        }
        entities.push(EntityOutput {
            name,
            entity_type,
            observations,
            truths,
            observation_count,
            observations_detailed,
        });
    }
    let mut relations = Vec::new();
    let mut rel_stmt = conn.prepare("SELECT from_entity, to_entity, relation_type FROM asobi_relations ORDER BY from_entity, to_entity, relation_type")?;
    for rel in rel_stmt.query_map([], |r| {
        Ok(RelationInput {
            from: r.get(0)?,
            to: r.get(1)?,
            relation_type: r.get(2)?,
        })
    })? {
        let rel = rel?;
        if values.is_empty()
            || entities
                .iter()
                .any(|e| e.name == rel.from || e.name == rel.to)
        {
            relations.push(rel);
        }
    }
    Ok(Graph {
        entities,
        relations,
    })
}

fn scoped_names(
    conn: &Connection,
    roots: &[String],
    rationale: bool,
) -> rusqlite::Result<Vec<String>> {
    let mut selected: HashSet<String> = roots.iter().map(|name| normalize(name)).collect();
    let relations = conn
        .prepare("SELECT from_entity, to_entity, relation_type FROM asobi_relations")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Expand only inward `part_of` edges, so selecting an epic never drags in
    // its parent project or a sibling epic.
    loop {
        let before = selected.len();
        for (from, to, kind) in &relations {
            if kind == "part_of" && selected.contains(to) {
                selected.insert(from.clone());
            }
        }
        if selected.len() == before {
            break;
        }
    }

    // Include one-hop rationale/dependency targets from the selected subtree.
    let cited: Vec<String> = relations
        .iter()
        .filter(|(from, _, kind)| {
            selected.contains(from) && (kind == "depends_on" || (rationale && kind == "extends"))
        })
        .map(|(_, to, _)| to.clone())
        .collect();
    selected.extend(cited);
    if rationale {
        let rationale_targets: Vec<String> = relations
            .iter()
            .filter(|(from, _, kind)| selected.contains(from) && kind == "extends")
            .map(|(_, to, _)| to.clone())
            .collect();
        selected.extend(rationale_targets);
    }
    let excluded: HashSet<String> = conn
        .prepare(
            "SELECT name FROM asobi_entities WHERE entity_type IN ('session','preference','standard')",
        )?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    selected.retain(|name| !excluded.contains(name));
    let mut names: Vec<_> = selected.into_iter().collect();
    names.sort();
    Ok(names)
}

impl GraphStore for SqliteStore {
    fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()> {
        self.write(|tx| {
            for entity in entities {
                let name = normalize(&entity.name);
                let inserted = tx.execute(
                    "INSERT OR IGNORE INTO asobi_entities(name, entity_type) VALUES (?, ?)",
                    params![name, entity.entity_type],
                )?;
                if inserted == 1 {
                    for content in entity.observations {
                        tx.execute(
                            "INSERT INTO asobi_observations(entity_name, content) VALUES (?, ?)",
                            params![normalize(&entity.name), content],
                        )?;
                    }
                }
            }
            Ok(())
        })
    }

    fn add_observations(&self, observations: Vec<ObservationInput>, limit: usize) -> ApiResult<()> {
        self.write(|tx| {
            for batch in observations {
                let entity = normalize(&batch.entity_name);
                for content in batch.contents { tx.execute("INSERT INTO asobi_observations(entity_name, content) VALUES (?, ?)", params![entity, content])?; }
                let cap = if limit == 0 { DEFAULT_OBSERVATION_LIMIT } else { limit };
                tx.execute("DELETE FROM asobi_observations WHERE entity_name = ? AND id NOT IN (SELECT id FROM asobi_observations WHERE entity_name = ? ORDER BY id DESC LIMIT ?)", params![entity, entity, cap as i64])?;
            }
            Ok(())
        })
    }

    fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        self.write(|tx| { for rel in relations { tx.execute("INSERT OR REPLACE INTO asobi_relations(from_entity,to_entity,relation_type) VALUES (?,?,?)", params![normalize(&rel.from), normalize(&rel.to), rel.relation_type])?; } Ok(()) })
    }
    fn delete_entities(&self, names: Vec<String>) -> ApiResult<()> {
        self.write(|tx| {
            for name in names {
                tx.execute(
                    "DELETE FROM asobi_entities WHERE name = ?",
                    [&normalize(&name)],
                )?;
            }
            Ok(())
        })
    }
    fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()> {
        self.write(|tx| {
            for deletion in deletions {
                for content in deletion.observations {
                    tx.execute(
                        "DELETE FROM asobi_observations WHERE entity_name = ? AND content = ?",
                        params![normalize(&deletion.entity_name), content],
                    )?;
                }
            }
            Ok(())
        })
    }
    fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM asobi_observations WHERE entity_name = ? AND id = ?",
                params![normalize(entity_name), id],
            )?;
            Ok(())
        })
    }
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE asobi_observations SET content = ? WHERE entity_name = ? AND id = ?",
                params![new_content, normalize(entity_name), id],
            )?;
            Ok(())
        })
    }
    fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE asobi_observations SET content = ? WHERE entity_name = ? AND content = ?",
                params![new_content, normalize(entity_name), old_content],
            )?;
            Ok(())
        })
    }
    fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        self.write(|tx| { for rel in relations { tx.execute("DELETE FROM asobi_relations WHERE from_entity = ? AND to_entity = ? AND relation_type = ?", params![normalize(&rel.from), normalize(&rel.to), rel.relation_type])?; } Ok(()) })
    }
    fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()> {
        self.write(|tx| { let entity = normalize(entity); tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,?,?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP", params![entity, key, value])?; Ok(()) })
    }
    fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()> {
        self.write(|tx| {
            tx.execute(
                "DELETE FROM asobi_truths WHERE entity_name = ? AND key = ?",
                params![normalize(entity), key],
            )?;
            Ok(())
        })
    }
    fn read_graph(&self) -> ApiResult<Graph> {
        self.graph(None, &[], false, 0)
    }
    fn read_graph_full(&self) -> ApiResult<Graph> {
        self.graph(None, &[], true, 0)
    }
    fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph> {
        self.read(|conn| {
            let names = scoped_names(conn, scope, rationale)?;
            let included: HashSet<_> = names.iter().cloned().collect();
            let mut graph = graph_from_connection(conn, Some(&names), &[], true, 0)?;
            graph
                .relations
                .retain(|rel| included.contains(&rel.from) && included.contains(&rel.to));
            Ok(graph)
        })
    }
    fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph> {
        self.graph(Some(&req.names), &req.expand, true, req.observation_limit)
    }
}

impl SearchStore for SqliteStore {
    fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph> {
        let term = query.query.trim().to_string();
        let limit = if query.limit == 0 { 100 } else { query.limit };
        self.read(|conn| {
            let mut names = Vec::new();
            let mut widened = false;
            if !term.is_empty() {
                let search_limit = if query.filters.is_empty() {
                    limit as i64
                } else {
                    -1
                };
                names = matching_names(conn, &term, search_limit)?;

                // FTS5 ANDs bare terms, and each index is searched separately,
                // so a question whose words are spread across an observation,
                // a truth and a name matches nothing at all -- and an empty
                // result is indistinguishable from "nothing was ever recorded".
                // That is the wrong way for a pitfall lookup to fail, so widen
                // to OR and let the caller know the query was loosened.
                if names.is_empty()
                    && let Some(widened_term) = widen_query(&term)
                {
                    names = matching_names(conn, &widened_term, search_limit)?;
                    widened = !names.is_empty();
                }
            }
            if !query.filters.is_empty() {
                let mut sql = String::from("SELECT e.name FROM asobi_entities e");
                let mut values = Vec::with_capacity(query.filters.len() * 2);
                for (idx, (key, value)) in query.filters.iter().enumerate() {
                    sql.push_str(&format!(" JOIN asobi_truths t{idx} ON t{idx}.entity_name=e.name AND t{idx}.key=? AND t{idx}.value=?"));
                    values.push(key.clone());
                    values.push(value.clone());
                }
                sql.push_str(" ORDER BY e.name");
                let mut stmt = conn.prepare(&sql)?;
                let eligible = stmt
                    .query_map(params_from_iter(values.iter()), |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
                if names.is_empty() {
                    names.extend(eligible);
                } else {
                    names.retain(|name| eligible.contains(name));
                }
            }
            names.truncate(limit);
            if widened {
                tracing::warn!(
                    "no exact match for {:?}; widened to any-term and found {}. \
                     Narrow with fewer words, or quote an exact phrase.",
                    term,
                    names.len()
                );
            }
            graph_from_connection(conn, Some(&names), &[], false, 0)
        })
    }
}

impl BackupStore for SqliteStore {
    fn backup(&self, request: BackupRequest) -> ApiResult<BackupReceipt> {
        let managed = request.destination.as_os_str().is_empty();
        let destination = if managed {
            let backup_dir = self
                .db_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).map_err(backend_error)?;
            backup_dir.join(format!("asobi-{}.db", backup_timestamp()?))
        } else {
            request.destination
        };
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(backend_error)?;
        }
        let escaped = destination.to_string_lossy().replace('\'', "''");
        self.read(|conn| conn.execute(&format!("VACUUM INTO '{}'", escaped), []))?;
        if managed {
            prune_managed_backups(
                destination.parent().unwrap_or_else(|| Path::new(".")),
                request.keep,
            )?;
        }
        Ok(BackupReceipt {
            path: destination,
            backend: "sqlite".into(),
        })
    }
    fn restore(self, source: PathBuf, force: bool) -> ApiResult<()> {
        if !source.exists() {
            return Err(ApiError::NotFound(source.display().to_string()));
        }
        let db_path = self.db_path.clone();
        if db_path.exists() && !force {
            return Err(ApiError::Conflict(format!(
                "database exists: {}",
                db_path.display()
            )));
        }
        let check = Connection::open(&source).map_err(backend_error)?;
        let result: String = check
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(backend_error)?;
        if result != "ok" {
            return Err(ApiError::Backend(format!(
                "backup integrity check failed: {result}"
            )));
        }
        let schema_version: i64 = check
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(backend_error)?;
        if schema_version != SCHEMA_VERSION {
            return Err(ApiError::Invalid(format!(
                "not an Asobi SQLite database: unsupported schema version {schema_version}"
            )));
        }
        for table in [
            "asobi_entities",
            "asobi_observations",
            "asobi_relations",
            "asobi_truths",
            "asobi_obs_fts",
        ] {
            let exists: bool = check
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ? AND type = 'table')",
                    [table],
                    |row| row.get(0),
                )
                .map_err(backend_error)?;
            if !exists {
                return Err(ApiError::Invalid(format!(
                    "not an Asobi SQLite database: missing {table}"
                )));
            }
        }
        drop(check);
        if db_path.exists() {
            self.read(|conn| conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);"))?;
            let backup_dir = db_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("backups");
            std::fs::create_dir_all(&backup_dir).map_err(backend_error)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(backend_error)?
                .as_nanos();
            std::fs::copy(&db_path, backup_dir.join(format!("pre-restore-{stamp}.db")))
                .map_err(backend_error)?;
        }
        drop(self);
        remove_database_sidecars(&db_path)?;
        std::fs::copy(source, db_path)
            .map(|_| ())
            .map_err(backend_error)
    }
}

fn backup_timestamp() -> ApiResult<u128> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(backend_error)?
        .as_nanos())
}

fn prune_managed_backups(directory: &Path, keep: usize) -> ApiResult<()> {
    let mut backups = std::fs::read_dir(directory)
        .map_err(backend_error)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("asobi-") && name.ends_with(".db")
        })
        .map(|entry| {
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (entry.path(), modified)
        })
        .collect::<Vec<_>>();
    backups.sort_by_key(|(_, modified)| Reverse(*modified));
    for (path, _) in backups.into_iter().skip(keep.max(1)) {
        std::fs::remove_file(path).map_err(backend_error)?;
    }
    Ok(())
}

fn remove_database_sidecars(path: &Path) -> ApiResult<()> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        match std::fs::remove_file(PathBuf::from(sidecar)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(backend_error(error)),
        }
    }
    Ok(())
}

impl MaintenanceStore for SqliteStore {
    fn stats(&self) -> ApiResult<Stats> {
        self.read(|conn| {
            Ok(Stats {
                entities: conn.query_row("SELECT count(*) FROM asobi_entities", [], |r| {
                    r.get::<_, i64>(0)
                })? as usize,
                relations: conn.query_row("SELECT count(*) FROM asobi_relations", [], |r| {
                    r.get::<_, i64>(0)
                })? as usize,
                observations: conn.query_row(
                    "SELECT count(*) FROM asobi_observations",
                    [],
                    |r| r.get::<_, i64>(0),
                )? as usize,
            })
        })
    }
    fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>> {
        self.read(|conn| { let mut stmt=conn.prepare("SELECT e.name,count(o.id) FROM asobi_entities e LEFT JOIN asobi_observations o ON o.entity_name=e.name GROUP BY e.name ORDER BY e.name")?; let mut out=Vec::new(); for row in stmt.query_map([],|r|Ok((r.get(0)?,r.get::<_,i64>(1)? as usize)))?{out.push(row?);} Ok(out) })
    }
    fn purge(&self, request: PurgeRequest) -> ApiResult<PurgeReport> {
        let report = self.write(|tx| {
            let candidates = collect_purge_candidates(tx, &request)?;
            let deleted = if request.apply {
                for candidate in &candidates {
                    tx.execute(
                        "DELETE FROM asobi_entities WHERE name = ?",
                        [&candidate.name],
                    )?;
                }
                candidates.len()
            } else {
                0
            };
            Ok(PurgeReport {
                dry_run: !request.apply,
                older_than_days: request.older_than_days,
                candidates,
                deleted,
            })
        })?;
        if report.deleted > 0 {
            // A no-op unless this database is in incremental auto-vacuum
            // mode, which every database is as of schema v5 -- see
            // `upgrade_to_v5`. Bounded to a few thousand pages so a large
            // backlog reclaims gradually across purges instead of stalling
            // this one; VACUUM (unbounded, exclusive-locking) stays a
            // manual `backup`-adjacent maintenance step, not something a
            // routine purge should pay for.
            self.read(|conn| conn.execute_batch("PRAGMA incremental_vacuum(2000);"))?;
        }
        Ok(report)
    }
    fn reset(&self) -> ApiResult<()> {
        self.write(|tx| { tx.execute_batch("DELETE FROM asobi_relations; DELETE FROM asobi_truths; DELETE FROM asobi_observations; DELETE FROM asobi_entities;")?; Ok(()) })?;
        self.read(|conn| conn.execute_batch("PRAGMA incremental_vacuum;"))?;
        Ok(())
    }
    fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        Ok(BackendCapabilities {
            backend: "sqlite".into(),
            keyword_search: true,
            keyword_search_kind: "fts5".into(),
            logical_snapshots: true,
            physical_backup: true,
            multi_process: true,
        })
    }
    fn health(&self) -> ApiResult<BackendHealth> {
        self.read(|conn| {
            conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?;
            Ok(BackendHealth {
                backend: "sqlite".into(),
                reachable: true,
                detail: Some(format!("schema {SCHEMA_VERSION}")),
            })
        })
    }
    fn location(&self) -> ApiResult<StorageLocation> {
        self.read(|conn| {
            let journal_mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
            Ok(StorageLocation {
                database_path: self.db_path.display().to_string(),
                journal_mode,
                schema_version: SCHEMA_VERSION as u32,
            })
        })
    }
}

impl TaskStore for SqliteStore {
    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> ApiResult<Option<String>> {
        self.write(|tx| {
            let target: Option<String> = match task {
                Some(task) => Some(normalize(task)),
                None => tx.query_row("SELECT t.entity_name FROM asobi_truths t JOIN asobi_entities e ON e.name=t.entity_name WHERE t.key='status' AND t.value='READY_TO_DISPATCH' AND e.entity_type='task' ORDER BY t.entity_name LIMIT 1", [], |r| r.get(0)).optional()?,
            };
            let Some(target) = target else { return Ok(None); };
            let changed = tx.execute("UPDATE asobi_truths SET value='DISPATCHED',updated_at=CURRENT_TIMESTAMP WHERE entity_name=? AND key='status' AND value='READY_TO_DISPATCH'", [&target])?;
            if changed == 0 { return Ok(None); }
            tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'claimed_by',?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP", params![target, agent])?;
            tx.execute("INSERT INTO asobi_observations(entity_name,content) VALUES (?,?)", params![target, format!("dispatched to {agent}")])?;
            let cap = if observation_limit == 0 { DEFAULT_OBSERVATION_LIMIT } else { observation_limit };
            tx.execute("DELETE FROM asobi_observations WHERE entity_name=? AND id NOT IN (SELECT id FROM asobi_observations WHERE entity_name=? ORDER BY id DESC LIMIT ?)", params![target, target, cap as i64])?;
            Ok(Some(target))
        })
    }
    fn claim_next(&self, agent: &str) -> ApiResult<Option<String>> {
        self.write(|tx| { let task:Option<String>=tx.query_row("SELECT t.entity_name FROM asobi_truths t JOIN asobi_entities e ON e.name=t.entity_name WHERE t.key='status' AND t.value='READY_TO_DISPATCH' AND e.entity_type='task' ORDER BY t.entity_name LIMIT 1",[],|r|r.get(0)).optional()?; if let Some(task)=task.as_ref(){tx.execute("UPDATE asobi_truths SET value='DISPATCHED',updated_at=CURRENT_TIMESTAMP WHERE entity_name=? AND key='status' AND value='READY_TO_DISPATCH'",[task])?;tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'claimed_by',?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP",params![task,agent])?;} Ok(task) })
    }
}
