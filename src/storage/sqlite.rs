//! The bundled SQLite provider behind the async `v3` traits, on sqlx (ADR 0008).
//!
//! One database file, WAL mode, incremental auto-vacuum, foreign keys on, a
//! bounded busy timeout. All provider detail -- schema, SQL, pragmas -- stays
//! inside this module; callers depend on the `api` traits only.
//!
//! sqlx specifics: queries are runtime-checked (`sqlx::query`, `query_as`;
//! never the `query!` macros, because part of the SQL is built dynamically and
//! the build must not need a database). One [`SqlitePool`] per graph file --
//! SQLite still has one writer, so concurrency comes from WAL readers and the
//! busy timeout, not from the pool. Transactions that must be atomic against
//! other writers -- task claims, abandonment, every mutation through
//! [`SqliteStore::begin_write`] -- begin with `BEGIN IMMEDIATE`, which the
//! default deferred transaction does not give.

use crate::api::v3::{
    ApiError, ApiResult, BackendCapabilities, BackendHealth, GraphStore, MaintenanceStore,
    OpenNodes, PurgeCandidate, PurgeReport, PurgeRequest, SearchQuery, SearchStore, Stats,
    StorageLocation, TaskStore,
};
use crate::model::{
    EntityInput, EntityOutput, Graph, ObservationDeletion, ObservationInput, RelationInput,
};
use sqlx::sqlite::{
    SqliteAutoVacuum, SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Connection, Row, Sqlite, SqliteConnection, SqlitePool, Transaction};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const SCHEMA_VERSION: u32 = 9;
const DEFAULT_DATABASE_FILENAME: &str = "asobi.db";
const DEFAULT_BUSY_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_OBSERVATION_LIMIT: usize = 200;
const PURGEABLE_ENTITY_TYPES: &[&str] = &["task"];
const PURGEABLE_STATUSES: &[&str] = &["DONE", "CLOSED", "ABANDONED"];

/// The statuses an abandonment sweep never touches; anything else -- including
/// no `status` truth at all -- counts as open.
const TERMINAL_STATUSES: &[&str] = &["DONE", "CLOSED", "ABANDONED"];

fn backend_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::Backend(error.to_string())
}

fn normalize(value: &str) -> String {
    crate::normalize::normalize_key(value)
}

/// Resolve a sweep window: environment variable first, then `asobi.toml`,
/// then the built-in default. The same order as `observation_limit`, the other
/// bound in this tool. `0` disables the window's behaviour at the call site.
fn resolve_window(env_key: &str, config: Option<u32>, default: u32) -> u32 {
    std::env::var(env_key)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .or(config)
        .unwrap_or(default)
}

/// The file's `user_version`, read over a plain read-only connection: opening
/// it writes nothing, which the pool's connection options (journal mode in
/// particular) cannot promise.
async fn read_user_version(path: &Path) -> Result<i64, sqlx::Error> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut conn = SqliteConnection::connect_with(&options).await?;
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut conn)
        .await
}

pub struct SqliteStore {
    pool: SqlitePool,
    db_path: PathBuf,
    /// Whether this process has already run the retention sweep.
    retention_swept: AtomicBool,
}

/// How long a finished task survives before the automatic sweep removes it.
///
/// Operational state is relevant for hours, occasionally days: a task that has
/// been `DONE` for a week is not context, it is archaeology. The previous
/// design left this to a manual `purge` that was correct in every respect
/// except that it never ran -- six weeks of daily use left a graph that was 96%
/// finished work. A default that has to be invoked is a default that does not
/// happen.
pub const DEFAULT_RETENTION_DAYS: u32 = 7;

/// How long an open task stays untouched before the sweep abandons it.
///
/// An abandoned task is terminal, so retention removes it `retention_days`
/// later: with both defaults, an idle task is visible as `ABANDONED` for a
/// week and gone the week after. The sweep records the observation that says
/// this happened, so the two-step lifecycle is always visible in the trail.
pub const DEFAULT_ABANDON_DAYS: u32 = 7;

/// What one automatic sweep did. Abandonment and retention are reported
/// separately because they are different lifecycles: the first marks idle open
/// work terminal, the second removes work that has been terminal for a while.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SweepReport {
    /// Open tasks the sweep marked `ABANDONED` this pass.
    pub abandoned: usize,
    /// Terminal entities retention deleted this pass.
    pub purged: usize,
}

impl SqliteStore {
    pub async fn open_default() -> crate::Result<Self> {
        let paths = crate::paths::AsobiPaths::resolve();
        let path = std::env::var(crate::paths::ENV_DATABASE_URL)
            .map(PathBuf::from)
            .unwrap_or_else(|_| paths.data_dir.join(DEFAULT_DATABASE_FILENAME));
        Self::open_at(&path).await
    }

    pub async fn open_at(path: &Path) -> crate::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Read the version before the pool opens: the pool's connection
        // options switch on WAL, which rewrites the file header, and a
        // refused database must be left byte-for-byte untouched.
        let version = if path.exists() {
            read_user_version(path).await.map_err(backend_error)?
        } else {
            0
        };
        if version != 0 {
            // Files this code creates are tracked by sqlx migrations and carry
            // user_version 0. A non-zero version is a hand-stamped file from
            // an earlier Asobi, and nothing is migrated into 0.8 (ADR 0007);
            // a version above ours is refused as too new rather than misread.
            if version < SCHEMA_VERSION as i64 {
                anyhow::bail!(
                    "{} was created by Asobi before 0.8 (schema {version}); \
                     pre-0.8 graph files are not migrated. Move the file aside to start a new graph there",
                    path.display()
                );
            }
            anyhow::bail!(
                "{} uses schema {version}, newer than this build's schema {SCHEMA_VERSION}; \
                 move the file aside and open it with a newer Asobi",
                path.display()
            );
        }

        let busy_timeout = Duration::from_millis(
            std::env::var("ASOBI_BUSY_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_BUSY_TIMEOUT_MS),
        );
        let mut options = SqliteConnectOptions::new()
            .filename(path)
            // The old driver created a missing file implicitly;
            // sqlx makes that explicit.
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(busy_timeout);
        if version == 0 && !path.exists() {
            // `auto_vacuum` only takes effect before the database file's
            // header is first written, so a genuinely new file asks for
            // INCREMENTAL through the connection options. Whether the option
            // ordering lands before the first write is verified by the
            // new-file test, not assumed; an existing schema-9 file already
            // has it and must not be vacuum-mode-switched underneath itself.
            options = options.auto_vacuum(SqliteAutoVacuum::Incremental);
        }
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(backend_error)?;

        // sqlx tracks applied migrations in its own table. The baseline is
        // idempotent -- every statement is IF NOT EXISTS -- so it is safe to
        // run on every open, and on every pool connection's schema view.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(backend_error)?;

        Ok(Self {
            pool,
            db_path: path.to_path_buf(),
            retention_swept: AtomicBool::new(false),
        })
    }

    /// One `BEGIN IMMEDIATE` transaction off the pool. Task claims,
    /// abandonment and every mutation go through here: a deferred transaction
    /// would let two processes take the same task before either writes.
    async fn begin_immediate(&self) -> ApiResult<Transaction<'static, Sqlite>> {
        self.pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend_error)
    }

    /// The once-per-process sweep, then the transaction. Reads never sweep.
    async fn begin_write(&self) -> ApiResult<Transaction<'static, Sqlite>> {
        self.sweep_expired_once().await;
        self.begin_immediate().await
    }

    /// Drop finished work once per process, before the first write, and
    /// abandon idle open tasks first.
    ///
    /// On a write rather than at open, so a pure read never mutates the graph
    /// -- `asobi show` must not delete anything. Once per process rather than
    /// per call, since a single command should not pay for the sweep
    /// repeatedly. Failures are ignored: the sweep is hygiene, and a command
    /// must not fail because housekeeping did.
    async fn sweep_expired_once(&self) {
        if self.retention_swept.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.sweep_body().await;
    }

    /// The sweep's body: abandonment, then retention. Everything underneath
    /// uses `begin_immediate` directly -- routing it back through
    /// `begin_write` would make this future recursive in its own type, which
    /// async fns cannot express; keeping the recursion out of the types (not
    /// boxing it away) is also what keeps the futures `Send` without
    /// `async_trait`.
    async fn sweep_body(&self) -> ApiResult<SweepReport> {
        let abandoned = self
            .abandon_idle_tasks(resolve_window(
                "ASOBI_ABANDON_DAYS",
                crate::paths::AsobiPaths::resolve().abandon_days,
                DEFAULT_ABANDON_DAYS,
            ))
            .await?;
        let retention_days = resolve_window(
            "ASOBI_RETENTION_DAYS",
            crate::paths::AsobiPaths::resolve().retention_days,
            DEFAULT_RETENTION_DAYS,
        );
        let purged = if retention_days == 0 {
            0
        } else {
            self.purge_inner(PurgeRequest {
                older_than_days: retention_days,
                apply: true,
            })
            .await?
            .deleted
        };
        Ok(SweepReport { abandoned, purged })
    }

    /// One full sweep: abandon idle open tasks, then let retention delete
    /// finished ones.
    ///
    /// Public and repeatable so the server (WP4) can run it on a background
    /// thread; the once-per-process guard lives in `sweep_expired_once`, not
    /// here. Abandonment runs first because it appends an observation, which
    /// refreshes `last_activity`: a newly abandoned task then survives
    /// retention for its full window instead of being marked and deleted in
    /// the same breath -- the two-step lifecycle the ADR promises.
    ///
    /// This is deliberately not part of the `v3` traits: the sweep is a
    /// provider-side maintenance behaviour, not a capability callers depend
    /// on, and it stays out of the RPC surface with them.
    pub async fn sweep(&self) -> ApiResult<SweepReport> {
        // A direct sweep must not run the once-per-process sweep inside this
        // one; setting the flag first is exactly what that guard checks.
        self.retention_swept.store(true, Ordering::SeqCst);
        self.sweep_body().await
    }

    /// Mark every idle open task `ABANDONED` in one transaction, each with an
    /// observation recording that it happened automatically.
    ///
    /// A task counts as idle when its `last_activity` -- maintained by the
    /// activity triggers over observations and truths -- is older than `days`,
    /// and it has no open `part_of` child: an epic's own entity goes quiet
    /// while its children are being worked, so the children's liveness
    /// protects it.
    async fn abandon_idle_tasks(&self, days: u32) -> ApiResult<usize> {
        if days == 0 {
            return Ok(0);
        }
        let mut tx = self.begin_immediate().await?;
        let status_placeholders = TERMINAL_STATUSES
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            r#"
            SELECT name FROM (
                SELECT e.name,
                    COALESCE(
                        (SELECT value FROM asobi_truths
                         WHERE entity_name = e.name AND key = 'status'),
                        ''
                    ) AS status,
                    e.last_activity AS last_activity
                FROM asobi_entities e
                WHERE e.entity_type = 'task'
            ) candidates
            WHERE status NOT IN ({status_placeholders})
              AND last_activity < datetime('now', ?)
              AND NOT EXISTS (
                  SELECT 1 FROM asobi_relations r
                  JOIN asobi_entities child ON child.name = r.from_entity
                  WHERE r.to_entity = candidates.name
                    AND r.relation_type = 'part_of'
                    AND COALESCE(
                            (SELECT value FROM asobi_truths
                             WHERE entity_name = child.name AND key = 'status'),
                            ''
                        ) NOT IN ({status_placeholders})
              )
            ORDER BY name
            "#
        );
        let cutoff = format!("-{days} days");
        // The status list is interpolated twice -- the candidate's own status
        // and the open-child check -- so it is bound twice too.
        let mut query = sqlx::query_scalar::<_, String>(&sql);
        for status in TERMINAL_STATUSES {
            query = query.bind(status);
        }
        query = query.bind(&cutoff);
        for status in TERMINAL_STATUSES {
            query = query.bind(status);
        }
        let names: Vec<String> = query.fetch_all(&mut *tx).await.map_err(backend_error)?;
        let observation = format!("abandoned automatically after {days} idle days");
        for name in &names {
            sqlx::query(
                "INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'status','ABANDONED') \
                 ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP",
            )
            .bind(name)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
            sqlx::query("INSERT INTO asobi_observations(entity_name, content) VALUES (?, ?)")
                .bind(name)
                .bind(&observation)
                .execute(&mut *tx)
                .await
                .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)?;
        Ok(names.len())
    }

    /// The lazy or eager graph read over one pooled connection.
    /// `observation_limit` of 0 means every observation; anything else returns
    /// the most recent N. `observationCount` stays the true total either way,
    /// so a caller can always tell what it is not being shown.
    async fn graph(
        &self,
        names: Option<&[String]>,
        expand: &[String],
        include_content: bool,
        observation_limit: usize,
    ) -> ApiResult<Graph> {
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        graph_from_connection(&mut conn, names, expand, include_content, observation_limit).await
    }

    async fn purge_inner(&self, request: PurgeRequest) -> ApiResult<PurgeReport> {
        let mut tx = self.begin_immediate().await?;
        let candidates = collect_purge_candidates(&mut tx, &request).await?;
        let deleted = if request.apply {
            for candidate in &candidates {
                sqlx::query("DELETE FROM asobi_entities WHERE name = ?")
                    .bind(&candidate.name)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend_error)?;
            }
            candidates.len()
        } else {
            0
        };
        tx.commit().await.map_err(backend_error)?;
        if deleted > 0 {
            // A no-op unless this database is in incremental auto-vacuum
            // mode, which every database is: `open_at` asks for it when it
            // creates the file. Bounded to a few thousand pages so a large
            // backlog reclaims gradually across purges instead of stalling
            // this one; VACUUM (unbounded, exclusive-locking) stays a manual
            // maintenance step, not something a routine purge should pay for.
            let mut conn = self.pool.acquire().await.map_err(backend_error)?;
            sqlx::query("PRAGMA incremental_vacuum(2000)")
                .execute(&mut *conn)
                .await
                .map_err(backend_error)?;
        }
        Ok(PurgeReport {
            dry_run: !request.apply,
            older_than_days: request.older_than_days,
            candidates,
            deleted,
        })
    }
}

/// Build the graph projection for the named entities (or all of them).
///
/// The caller's ordering is preserved and meaningful: for `search` it is the
/// fused relevance ranking, and for `show` it is the order the names were
/// asked for. `IN (...)` returns rows in whatever order SQLite likes, so the
/// caller's order is restored afterwards -- sorting by name here silently
/// discarded every ranking the search had just computed.
async fn graph_from_connection(
    conn: &mut SqliteConnection,
    names: Option<&[String]>,
    expand: &[String],
    include_content: bool,
    observation_limit: usize,
) -> ApiResult<Graph> {
    let mut selected = names.map(|values| values.iter().map(|v| normalize(v)).collect::<Vec<_>>());
    if let Some(values) = selected.as_mut()
        && !expand.is_empty()
        && !values.is_empty()
    {
        let placeholders = (0..values.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT from_entity, to_entity, relation_type FROM asobi_relations \
             WHERE from_entity IN ({placeholders}) OR to_entity IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql);
        for value in values.iter() {
            query = query.bind(value);
        }
        for value in values.iter() {
            query = query.bind(value);
        }
        let rows = query.fetch_all(&mut *conn).await.map_err(backend_error)?;
        for row in rows {
            let from: String = row.try_get(0).map_err(backend_error)?;
            let to: String = row.try_get(1).map_err(backend_error)?;
            let kind: String = row.try_get(2).map_err(backend_error)?;
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
    let mut entity_query = sqlx::query(&entity_sql);
    if entity_sql.contains("IN (") {
        for value in &values {
            entity_query = entity_query.bind(value);
        }
    }
    let mut entity_rows: Vec<(String, String)> = entity_query
        .fetch_all(&mut *conn)
        .await
        .map_err(backend_error)?
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>(0).map_err(backend_error)?,
                row.try_get::<String, _>(1).map_err(backend_error)?,
            ))
        })
        .collect::<Result<_, ApiError>>()?;
    let position: std::collections::HashMap<&str, usize> = values
        .iter()
        .enumerate()
        .map(|(index, name)| (name.as_str(), index))
        .collect();
    entity_rows.sort_by_key(|(name, _)| position.get(name.as_str()).copied().unwrap_or(usize::MAX));

    let mut entities = Vec::new();
    for (name, entity_type) in entity_rows {
        // Prepared separately and fetched per entity: the true total is what
        // tells a caller how much a limited read left behind.
        let observation_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM asobi_observations WHERE entity_name = ?")
                .bind(&name)
                .fetch_one(&mut *conn)
                .await
                .map_err(backend_error)?;
        let (observations, observations_detailed) = if include_content {
            // Newest-first inside the limit, then re-ordered oldest-first so a
            // truncated trail still reads in the direction it was written.
            let cap = if observation_limit == 0 {
                -1 // SQLite: negative LIMIT is unbounded
            } else {
                observation_limit as i64
            };
            let rows = sqlx::query(
                "SELECT id, content FROM (
                     SELECT id, content FROM asobi_observations
                     WHERE entity_name = ? ORDER BY id DESC LIMIT ?
                 ) ORDER BY id",
            )
            .bind(&name)
            .bind(cap)
            .fetch_all(&mut *conn)
            .await
            .map_err(backend_error)?;
            let mut observations = Vec::new();
            let mut detailed = Vec::new();
            for row in rows {
                let id: i64 = row.try_get(0).map_err(backend_error)?;
                let content: String = row.try_get(1).map_err(backend_error)?;
                observations.push(content.clone());
                detailed.push(crate::model::DetailedObservation { id, content });
            }
            (observations, Some(detailed))
        } else {
            (Vec::new(), None)
        };
        let truths: BTreeMap<String, String> =
            sqlx::query("SELECT key, value FROM asobi_truths WHERE entity_name = ? ORDER BY key")
                .bind(&name)
                .fetch_all(&mut *conn)
                .await
                .map_err(backend_error)?
                .into_iter()
                .filter_map(|row| {
                    let key = row.try_get::<String, _>(0).ok()?;
                    let value = row.try_get::<String, _>(1).ok()?;
                    Some((key, value))
                })
                .collect();
        entities.push(EntityOutput {
            name,
            entity_type,
            observations,
            truths,
            observation_count: observation_count as usize,
            observations_detailed,
        });
    }
    let relations: Vec<RelationInput> = sqlx::query(
        "SELECT from_entity, to_entity, relation_type FROM asobi_relations \
         ORDER BY from_entity, to_entity, relation_type",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(backend_error)?
    .into_iter()
    .map(|row| {
        Ok(RelationInput {
            from: row.try_get(0).map_err(backend_error)?,
            to: row.try_get(1).map_err(backend_error)?,
            relation_type: row.try_get(2).map_err(backend_error)?,
        })
    })
    .collect::<Result<_, ApiError>>()?;
    let relations = relations
        .into_iter()
        .filter(|rel| {
            values.is_empty()
                || entities
                    .iter()
                    .any(|e| e.name == rel.from || e.name == rel.to)
        })
        .collect();
    Ok(Graph {
        entities,
        relations,
    })
}

/// Entities matching `term`, across every path Asobi indexes: observation
/// text, truth values, and the entity's own name or type, fused by reciprocal
/// rank.
///
/// Truth values were unsearchable before 0.7, which mattered because the
/// convention is to store a pitfall's human-readable warning in a `title`
/// truth -- so the one sentence explaining a dead end was the one thing recall
/// could not reach.
async fn matching_names(
    conn: &mut SqliteConnection,
    term: &str,
    limit: i64,
) -> ApiResult<Vec<String>> {
    // A negative limit means "unbounded" (a filtered search post-filters), and
    // the pool has to be at least as generous as the caller's request.
    let pool = if limit < 0 {
        -1
    } else {
        limit.max(CANDIDATE_POOL)
    };
    let like = format!("%{term}%");
    // One ranked query per path. A malformed FTS5 query is a user error, not
    // a failure: the other paths still answer, and the LIKE fallback usually
    // does -- so its errors collapse to "no matches on this path".
    async fn ranked(
        conn: &mut SqliteConnection,
        sql: &str,
        binds: &[String],
    ) -> ApiResult<Vec<String>> {
        let mut query = sqlx::query_scalar::<_, String>(sql);
        for bind in binds {
            query = query.bind(bind);
        }
        Ok(query.fetch_all(&mut *conn).await.unwrap_or_default())
    }
    let fts_entities = "SELECT DISTINCT o.entity_name FROM asobi_obs_fts
        JOIN asobi_observations o ON asobi_obs_fts.rowid = o.rowid
        WHERE asobi_obs_fts MATCH ? ORDER BY bm25(asobi_obs_fts) LIMIT ?";
    let fts_truths = "SELECT DISTINCT t.entity_name FROM asobi_truth_fts
        JOIN asobi_truths t ON asobi_truth_fts.rowid = t.rowid
        WHERE asobi_truth_fts MATCH ? ORDER BY bm25(asobi_truth_fts) LIMIT ?";
    let fts_names = "SELECT name FROM asobi_entities
        WHERE name LIKE ? OR entity_type LIKE ? ORDER BY name LIMIT ?";
    let pool_string = pool.to_string();
    let paths = [
        ranked(conn, fts_entities, &[term.to_string(), pool_string.clone()]).await?,
        ranked(conn, fts_truths, &[term.to_string(), pool_string.clone()]).await?,
        ranked(
            conn,
            fts_names,
            &[like.clone(), like.clone(), pool_string.clone()],
        )
        .await?,
    ];

    // Reciprocal rank fusion across the three paths.
    //
    // Concatenating them meant every observation match outranked every truth
    // match, so an entity found only by its `title` landed last however good
    // the match was -- and the ordering shifted with `--limit`, since each
    // path was capped before the merge. Fusing by rank instead lets a strong
    // match in one path compete with a strong match in another, and rewards
    // an entity that several paths agree on. The constant 60 is the
    // conventional RRF damping value: large enough that the top few ranks are
    // not runaway favourites, small enough that rank still dominates.
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

/// How many candidates each path contributes to the fusion, before the caller
/// truncates to its own limit.
///
/// Fixed rather than derived from `--limit`, so ranking does not shift when
/// the caller asks for more: capping each path at the requested limit meant a
/// wider request pulled in weaker candidates that diluted the fused order, and
/// the same entity moved position depending on how many results were asked
/// for.
const CANDIDATE_POOL: i64 = 100;

/// Rewrite a multi-word query as `a OR b OR c`, or `None` when there is
/// nothing to widen -- a single term, or one already carrying FTS5 operators,
/// where rewriting would either change nothing or corrupt the caller's intent.
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

async fn collect_purge_candidates(
    conn: &mut SqliteConnection,
    request: &PurgeRequest,
) -> ApiResult<Vec<PurgeCandidate>> {
    let type_placeholders = TERMINAL_STATUSES
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let _ = &type_placeholders;
    let type_list = PURGEABLE_ENTITY_TYPES
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let status_list = PURGEABLE_STATUSES
        .iter()
        .map(|_| "?")
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
                e.last_activity AS last_activity,
                (SELECT COUNT(*) FROM asobi_observations
                 WHERE entity_name = e.name) AS observations,
                (SELECT COUNT(*) FROM asobi_relations
                 WHERE from_entity = e.name OR to_entity = e.name) AS relations
            FROM asobi_entities e
            WHERE e.entity_type IN ({type_list})
        ) candidates
        WHERE status IN ({status_list})
          AND last_activity < datetime('now', ?)
        ORDER BY last_activity, name
        "#
    );
    let cutoff = format!("-{} days", request.older_than_days);
    let mut query = sqlx::query(&sql);
    for value in PURGEABLE_ENTITY_TYPES {
        query = query.bind(value);
    }
    for value in PURGEABLE_STATUSES {
        query = query.bind(value);
    }
    query = query.bind(&cutoff);
    let _ = &type_placeholders;
    let candidates = query
        .fetch_all(&mut *conn)
        .await
        .map_err(backend_error)?
        .into_iter()
        .map(|row| {
            Ok(PurgeCandidate {
                name: row.try_get(0).map_err(backend_error)?,
                entity_type: row.try_get(1).map_err(backend_error)?,
                status: row.try_get(2).map_err(backend_error)?,
                last_activity: row.try_get(3).map_err(backend_error)?,
                observations: row.try_get::<i64, _>(4).map_err(backend_error)? as usize,
                relations: row.try_get::<i64, _>(5).map_err(backend_error)? as usize,
            })
        })
        .collect::<Result<_, ApiError>>()?;
    Ok(candidates)
}

impl GraphStore for SqliteStore {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for entity in entities {
            let name = normalize(&entity.name);
            let inserted = sqlx::query(
                "INSERT OR IGNORE INTO asobi_entities(name, entity_type) VALUES (?, ?)",
            )
            .bind(&name)
            .bind(&entity.entity_type)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?
            .rows_affected();
            if inserted == 1 {
                for content in entity.observations {
                    sqlx::query(
                        "INSERT INTO asobi_observations(entity_name, content) VALUES (?, ?)",
                    )
                    .bind(&name)
                    .bind(&content)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend_error)?;
                }
            }
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for batch in observations {
            let entity = normalize(&batch.entity_name);
            for content in batch.contents {
                sqlx::query("INSERT INTO asobi_observations(entity_name, content) VALUES (?, ?)")
                    .bind(&entity)
                    .bind(&content)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend_error)?;
            }
            let cap = if limit == 0 {
                DEFAULT_OBSERVATION_LIMIT
            } else {
                limit
            };
            sqlx::query(
                "DELETE FROM asobi_observations WHERE entity_name = ? AND id NOT IN \
                 (SELECT id FROM asobi_observations WHERE entity_name = ? ORDER BY id DESC LIMIT ?)",
            )
            .bind(&entity)
            .bind(&entity)
            .bind(cap as i64)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for rel in relations {
            sqlx::query(
                "INSERT OR REPLACE INTO asobi_relations(from_entity,to_entity,relation_type) VALUES (?,?,?)",
            )
            .bind(normalize(&rel.from))
            .bind(normalize(&rel.to))
            .bind(&rel.relation_type)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn delete_entities(&self, names: Vec<String>) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for name in names {
            sqlx::query("DELETE FROM asobi_entities WHERE name = ?")
                .bind(normalize(&name))
                .execute(&mut *tx)
                .await
                .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for deletion in deletions {
            for content in deletion.observations {
                sqlx::query("DELETE FROM asobi_observations WHERE entity_name = ? AND content = ?")
                    .bind(normalize(&deletion.entity_name))
                    .bind(&content)
                    .execute(&mut *tx)
                    .await
                    .map_err(backend_error)?;
            }
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM asobi_observations WHERE entity_name = ? AND id = ?")
            .bind(normalize(entity_name))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)
    }

    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        sqlx::query("UPDATE asobi_observations SET content = ? WHERE entity_name = ? AND id = ?")
            .bind(new_content)
            .bind(normalize(entity_name))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)
    }

    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        sqlx::query(
            "UPDATE asobi_observations SET content = ? WHERE entity_name = ? AND content = ?",
        )
        .bind(new_content)
        .bind(normalize(entity_name))
        .bind(old_content)
        .execute(&mut *tx)
        .await
        .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)
    }

    async fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        for rel in relations {
            sqlx::query(
                "DELETE FROM asobi_relations WHERE from_entity = ? AND to_entity = ? AND relation_type = ?",
            )
            .bind(normalize(&rel.from))
            .bind(normalize(&rel.to))
            .bind(&rel.relation_type)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)
    }

    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        let entity = normalize(entity);
        sqlx::query(
            "INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,?,?) \
             ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP",
        )
        .bind(&entity)
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await
        .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)
    }

    async fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM asobi_truths WHERE entity_name = ? AND key = ?")
            .bind(normalize(entity))
            .bind(key)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)
    }

    async fn read_graph(&self) -> ApiResult<Graph> {
        self.graph(None, &[], false, 0).await
    }

    async fn read_graph_full(&self) -> ApiResult<Graph> {
        self.graph(None, &[], true, 0).await
    }

    async fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph> {
        self.graph(Some(&req.names), &req.expand, true, req.observation_limit)
            .await
    }
}

impl SearchStore for SqliteStore {
    async fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph> {
        let term = query.query.trim().to_string();
        let limit = if query.limit == 0 { 100 } else { query.limit };
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        let mut names = Vec::new();
        let mut widened = false;
        if !term.is_empty() {
            let search_limit = if query.filters.is_empty() {
                limit as i64
            } else {
                -1
            };
            names = matching_names(&mut conn, &term, search_limit).await?;

            // FTS5 ANDs bare terms, and each index is searched separately, so
            // a question whose words are spread across an observation, a truth
            // and a name matches nothing at all -- and an empty result is
            // indistinguishable from "nothing was ever recorded". That is the
            // wrong way for a pitfall lookup to fail, so widen to OR and let
            // the caller know the query was loosened.
            if names.is_empty()
                && let Some(widened_term) = widen_query(&term)
            {
                names = matching_names(&mut conn, &widened_term, search_limit).await?;
                widened = !names.is_empty();
            }
        }
        if !query.filters.is_empty() {
            let mut sql = String::from("SELECT e.name FROM asobi_entities e");
            let mut binds = Vec::with_capacity(query.filters.len() * 2);
            for (idx, (key, value)) in query.filters.iter().enumerate() {
                sql.push_str(&format!(
                    " JOIN asobi_truths t{idx} ON t{idx}.entity_name=e.name AND t{idx}.key=? AND t{idx}.value=?"
                ));
                binds.push(key.clone());
                binds.push(value.clone());
            }
            sql.push_str(" ORDER BY e.name");
            let mut filter_query = sqlx::query(&sql);
            for bind in &binds {
                filter_query = filter_query.bind(bind);
            }
            let eligible: std::collections::HashSet<String> = filter_query
                .fetch_all(&mut *conn)
                .await
                .map_err(backend_error)?
                .into_iter()
                .filter_map(|row| row.try_get::<String, _>(0).ok())
                .collect();
            if names.is_empty() {
                names.extend(eligible);
            } else {
                names.retain(|name| eligible.contains(name));
            }
        }
        names.truncate(limit);
        if widened {
            tracing::warn!(
                "no exact match for {term:?}; widened to any-term and found {}. \
                 Narrow with fewer words, or quote an exact phrase.",
                names.len()
            );
        }
        graph_from_connection(&mut conn, Some(&names), &[], false, 0).await
    }
}

impl MaintenanceStore for SqliteStore {
    async fn stats(&self) -> ApiResult<Stats> {
        let count = |sql: &'static str| async {
            let mut conn = self.pool.acquire().await.map_err(backend_error)?;
            let count: i64 = sqlx::query_scalar(sql)
                .fetch_one(&mut *conn)
                .await
                .map_err(backend_error)?;
            Ok::<usize, ApiError>(count as usize)
        };
        Ok(Stats {
            entities: count("SELECT count(*) FROM asobi_entities").await?,
            relations: count("SELECT count(*) FROM asobi_relations").await?,
            observations: count("SELECT count(*) FROM asobi_observations").await?,
        })
    }

    async fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>> {
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        let rows = sqlx::query(
            "SELECT e.name, count(o.id) FROM asobi_entities e \
             LEFT JOIN asobi_observations o ON o.entity_name = e.name \
             GROUP BY e.name ORDER BY e.name",
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(backend_error)?;
        Ok(rows
            .into_iter()
            .filter_map(|row| {
                let name = row.try_get::<String, _>(0).ok()?;
                let count = row.try_get::<i64, _>(1).ok()? as usize;
                Some((name, count))
            })
            .collect())
    }

    async fn purge(&self, request: PurgeRequest) -> ApiResult<PurgeReport> {
        self.sweep_expired_once().await;
        self.purge_inner(request).await
    }

    async fn reset(&self) -> ApiResult<()> {
        let mut tx = self.begin_write().await?;
        sqlx::query("DELETE FROM asobi_relations")
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        sqlx::query("DELETE FROM asobi_truths")
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        sqlx::query("DELETE FROM asobi_observations")
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        sqlx::query("DELETE FROM asobi_entities")
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)?;
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        sqlx::query("PRAGMA incremental_vacuum")
            .execute(&mut *conn)
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        Ok(BackendCapabilities {
            backend: "sqlite".into(),
            keyword_search: true,
            keyword_search_kind: "fts5".into(),
            multi_process: true,
        })
    }

    async fn health(&self) -> ApiResult<BackendHealth> {
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        let _: i64 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&mut *conn)
            .await
            .map_err(backend_error)?;
        Ok(BackendHealth {
            backend: "sqlite".into(),
            reachable: true,
            detail: Some(format!("schema {SCHEMA_VERSION}")),
        })
    }

    async fn location(&self) -> ApiResult<StorageLocation> {
        let mut conn = self.pool.acquire().await.map_err(backend_error)?;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&mut *conn)
            .await
            .map_err(backend_error)?;
        Ok(StorageLocation {
            database_path: self.db_path.display().to_string(),
            journal_mode,
            schema_version: SCHEMA_VERSION,
        })
    }
}

impl TaskStore for SqliteStore {
    async fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> ApiResult<Option<String>> {
        let mut tx = self.begin_write().await?;
        let target: Option<String> = match task {
            Some(task) => Some(normalize(task)),
            None => sqlx::query_scalar(
                "SELECT t.entity_name FROM asobi_truths t \
                 JOIN asobi_entities e ON e.name = t.entity_name \
                 WHERE t.key = 'status' AND t.value = 'READY_TO_DISPATCH' \
                   AND e.entity_type = 'task' \
                 ORDER BY t.entity_name LIMIT 1",
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend_error)?,
        };
        let Some(target) = target else {
            return Ok(None);
        };
        let changed = sqlx::query(
            "UPDATE asobi_truths SET value='DISPATCHED', updated_at=CURRENT_TIMESTAMP \
             WHERE entity_name=? AND key='status' AND value='READY_TO_DISPATCH'",
        )
        .bind(&target)
        .execute(&mut *tx)
        .await
        .map_err(backend_error)?
        .rows_affected();
        if changed == 0 {
            return Ok(None);
        }
        sqlx::query(
            "INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'claimed_by',?) \
             ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP",
        )
        .bind(&target)
        .bind(agent)
        .execute(&mut *tx)
        .await
        .map_err(backend_error)?;
        sqlx::query("INSERT INTO asobi_observations(entity_name,content) VALUES (?,?)")
            .bind(&target)
            .bind(format!("dispatched to {agent}"))
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        let cap = if observation_limit == 0 {
            DEFAULT_OBSERVATION_LIMIT
        } else {
            observation_limit
        };
        sqlx::query(
            "DELETE FROM asobi_observations WHERE entity_name=? AND id NOT IN \
             (SELECT id FROM asobi_observations WHERE entity_name=? ORDER BY id DESC LIMIT ?)",
        )
        .bind(&target)
        .bind(&target)
        .bind(cap as i64)
        .execute(&mut *tx)
        .await
        .map_err(backend_error)?;
        tx.commit().await.map_err(backend_error)?;
        Ok(Some(target))
    }

    async fn claim_next(&self, agent: &str) -> ApiResult<Option<String>> {
        let mut tx = self.begin_write().await?;
        let task: Option<String> = sqlx::query_scalar(
            "SELECT t.entity_name FROM asobi_truths t \
             JOIN asobi_entities e ON e.name = t.entity_name \
             WHERE t.key = 'status' AND t.value = 'READY_TO_DISPATCH' \
               AND e.entity_type = 'task' \
             ORDER BY t.entity_name LIMIT 1",
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend_error)?;
        if let Some(task) = task.as_ref() {
            let changed = sqlx::query(
                "UPDATE asobi_truths SET value='DISPATCHED', updated_at=CURRENT_TIMESTAMP \
                 WHERE entity_name=? AND key='status' AND value='READY_TO_DISPATCH'",
            )
            .bind(task)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?
            .rows_affected();
            if changed == 0 {
                return Ok(None);
            }
            sqlx::query(
                "INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'claimed_by',?) \
                 ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP",
            )
            .bind(task)
            .bind(agent)
            .execute(&mut *tx)
            .await
            .map_err(backend_error)?;
        }
        tx.commit().await.map_err(backend_error)?;
        Ok(task)
    }
}
