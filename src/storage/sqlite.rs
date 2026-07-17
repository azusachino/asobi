use crate::api::v2::{
    ApiError, ApiResult, BackendCapabilities, BackendHealth, BackupReceipt, BackupRequest,
    BackupStore, GraphStore, ImportReport, MaintenanceStore, OpenNodes, SearchQuery, SearchStore,
    SkillRecord, SkillStore, Snapshot, SnapshotStore, Stats, StorageLocation, TaskStore,
    TruthVersion,
};
use crate::model::{
    EntityInput, EntityOutput, Graph, ObservationDeletion, ObservationInput, RelationInput,
};
use rusqlite::{
    Connection, OptionalExtension, Transaction, TransactionBehavior, params, params_from_iter,
};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const SCHEMA_VERSION: i64 = 4;
const DEFAULT_DATABASE_FILENAME: &str = "asobi.db";
const DEFAULT_BUSY_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_OBSERVATION_LIMIT: usize = 200;

fn backend_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::Backend(error.to_string())
}

fn normalize(value: &str) -> String {
    crate::normalize::normalize_key(value)
}

pub struct SqliteStore {
    conn: Mutex<Connection>,
    db_path: PathBuf,
}

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
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        Self::init_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: path.to_path_buf(),
        })
    }

    fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
        let previous_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
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
            CREATE TABLE IF NOT EXISTS asobi_truth_history (
                entity_name TEXT NOT NULL REFERENCES asobi_entities(name) ON DELETE CASCADE,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                valid_from TEXT NOT NULL,
                valid_until TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_truth_history ON asobi_truth_history(entity_name, key, valid_until);
            CREATE TABLE IF NOT EXISTS asobi_skills (
                entity_name TEXT PRIMARY KEY REFERENCES asobi_entities(name) ON DELETE CASCADE,
                body TEXT NOT NULL,
                source TEXT NOT NULL,
                version TEXT NOT NULL,
                installed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
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
            PRAGMA user_version = 4;",
        )?;
        let count: i64 =
            conn.query_row("SELECT count(*) FROM asobi_observations", [], |r| r.get(0))?;
        if count > 0 && previous_version < SCHEMA_VERSION {
            let _ = conn.execute(
                "INSERT INTO asobi_obs_fts(asobi_obs_fts) VALUES ('rebuild')",
                [],
            );
        }
        Ok(())
    }

    fn write<T>(
        &self,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> ApiResult<T> {
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

    fn graph(
        &self,
        names: Option<&[String]>,
        expand: &[String],
        include_content: bool,
    ) -> ApiResult<Graph> {
        self.read(|conn| graph_from_connection(conn, names, expand, include_content))
    }
}

fn graph_from_connection(
    conn: &Connection,
    names: Option<&[String]>,
    expand: &[String],
    include_content: bool,
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
            "SELECT name, entity_type FROM asobi_entities WHERE name IN ({}) ORDER BY name",
            (0..values.len()).map(|_| "?").collect::<Vec<_>>().join(",")
        ),
        None => "SELECT name, entity_type FROM asobi_entities ORDER BY name".to_string(),
    };
    let values = selected.unwrap_or_default();
    let entity_rows: Vec<(String, String)> = if entity_sql.contains("IN (") {
        let mut stmt = conn.prepare(&entity_sql)?;
        stmt.query_map(rusqlite::params_from_iter(values.iter()), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut stmt = conn.prepare(&entity_sql)?;
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut entities = Vec::new();
    let mut obs_stmt = if include_content {
        Some(conn.prepare(
            "SELECT id, content FROM asobi_observations WHERE entity_name = ? ORDER BY id",
        )?)
    } else {
        None
    };
    let mut obs_count_stmt = if include_content {
        None
    } else {
        Some(conn.prepare("SELECT COUNT(*) FROM asobi_observations WHERE entity_name = ?")?)
    };
    let mut truth_stmt =
        conn.prepare("SELECT key, value FROM asobi_truths WHERE entity_name = ? ORDER BY key")?;
    for (name, entity_type) in entity_rows {
        let (observations, observations_detailed, observation_count) = if include_content {
            let mut observations = Vec::new();
            let mut detailed = Vec::new();
            for obs in obs_stmt
                .as_mut()
                .expect("content query must be prepared")
                .query_map([&name], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
            {
                let (id, content) = obs?;
                observations.push(content.clone());
                detailed.push(crate::model::DetailedObservation { id, content });
            }
            let observation_count = detailed.len();
            (observations, Some(detailed), observation_count)
        } else {
            let count = obs_count_stmt
                .as_mut()
                .expect("count query must be prepared")
                .query_row([&name], |r| r.get::<_, i64>(0))? as usize;
            (Vec::new(), None, count)
        };
        let mut truths = BTreeMap::new();
        for truth in truth_stmt.query_map([&name], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (key, value) = truth?;
            truths.insert(key, value);
        }
        let body = if include_content {
            conn.query_row(
                "SELECT body FROM asobi_skills WHERE entity_name = ?",
                [&name],
                |r| r.get(0),
            )
            .optional()?
        } else {
            None
        };
        entities.push(EntityOutput {
            name,
            entity_type,
            observations,
            truths,
            observation_count,
            body,
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
        self.write(|tx| { let entity = normalize(entity); tx.execute("INSERT INTO asobi_truth_history(entity_name,key,value,valid_from,valid_until) SELECT entity_name,key,value,updated_at,CURRENT_TIMESTAMP FROM asobi_truths WHERE entity_name=? AND key=? AND value<>?", params![entity, key, value])?; tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,?,?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value, updated_at=CURRENT_TIMESTAMP", params![entity, key, value])?; Ok(()) })
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
    fn truth_history(&self, entity: &str, key: Option<&str>) -> ApiResult<Vec<TruthVersion>> {
        self.read(|conn| { let mut out = Vec::new(); if let Some(key) = key { let mut stmt = conn.prepare("SELECT key,value,valid_from,valid_until FROM asobi_truth_history WHERE entity_name=? AND key=? ORDER BY valid_until DESC")?; for row in stmt.query_map(params![normalize(entity), key], |r| Ok(TruthVersion { key:r.get(0)?, value:r.get(1)?, valid_from:r.get(2)?, valid_until:r.get(3)? }))? { out.push(row?); } } else { let mut stmt = conn.prepare("SELECT key,value,valid_from,valid_until FROM asobi_truth_history WHERE entity_name=? ORDER BY valid_until DESC,key")?; for row in stmt.query_map([normalize(entity)], |r| Ok(TruthVersion { key:r.get(0)?, value:r.get(1)?, valid_from:r.get(2)?, valid_until:r.get(3)? }))? { out.push(row?); } } Ok(out) })
    }
    fn read_graph(&self) -> ApiResult<Graph> {
        self.graph(None, &[], false)
    }
    fn read_graph_full(&self) -> ApiResult<Graph> {
        self.graph(None, &[], true)
    }
    fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph> {
        self.read(|conn| {
            let names = scoped_names(conn, scope, rationale)?;
            let included: HashSet<_> = names.iter().cloned().collect();
            let mut graph = graph_from_connection(conn, Some(&names), &[], true)?;
            graph
                .relations
                .retain(|rel| included.contains(&rel.from) && included.contains(&rel.to));
            Ok(graph)
        })
    }
    fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph> {
        self.graph(Some(&req.names), &req.expand, true)
    }
}

impl SearchStore for SqliteStore {
    fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph> {
        let term = query.query.trim().to_string();
        let limit = if query.limit == 0 { 100 } else { query.limit };
        self.read(|conn| {
            let mut names = Vec::new();
            if !term.is_empty() {
                let search_limit = if query.filters.is_empty() {
                    limit as i64
                } else {
                    -1
                };
                let mut stmt = conn.prepare("SELECT DISTINCT o.entity_name FROM asobi_obs_fts JOIN asobi_observations o ON asobi_obs_fts.rowid=o.rowid WHERE asobi_obs_fts MATCH ? ORDER BY bm25(asobi_obs_fts) LIMIT ?")?;
                if let Ok(rows) = stmt.query_map(params![term, search_limit], |r| r.get::<_, String>(0)) {
                    for row in rows.flatten() {
                        names.push(row);
                    }
                }
                let like = format!("%{}%", term);
                let mut stmt = conn.prepare("SELECT name FROM asobi_entities WHERE name LIKE ? OR entity_type LIKE ? ORDER BY name LIMIT ?")?;
                for row in stmt.query_map(params![like, format!("%{}%", term), search_limit], |r| r.get::<_, String>(0))? { let name = row?; if !names.contains(&name) { names.push(name); } }
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
            graph_from_connection(conn, Some(&names), &[], false)
        })
    }
}

impl SkillStore for SqliteStore {
    fn list_skills(&self) -> ApiResult<Vec<SkillRecord>> {
        self.read(|conn| { let mut stmt = conn.prepare("SELECT s.entity_name,s.body,s.source,s.version,COALESCE(t.value,'') FROM asobi_skills s LEFT JOIN asobi_truths t ON t.entity_name=s.entity_name AND t.key='description' ORDER BY s.source,s.entity_name")?; let mut out = Vec::new(); for row in stmt.query_map([], |r| Ok(SkillRecord { entity_name:r.get(0)?, body:r.get(1)?, source:r.get(2)?, version:r.get(3)?, description:r.get(4)? }))? { out.push(row?); } Ok(out) })
    }
    fn skill_body(&self, entity_name: &str) -> ApiResult<Option<String>> {
        self.read(|conn| {
            conn.query_row(
                "SELECT body FROM asobi_skills WHERE entity_name=?",
                [normalize(entity_name)],
                |r| r.get(0),
            )
            .optional()
        })
    }
    fn upsert_skill(&self, skill: SkillRecord) -> ApiResult<()> {
        self.write(|tx| { let name = normalize(&skill.entity_name); tx.execute("INSERT OR IGNORE INTO asobi_entities(name,entity_type) VALUES (?, 'skill')", [&name])?; tx.execute("INSERT INTO asobi_skills(entity_name,body,source,version) VALUES (?,?,?,?) ON CONFLICT(entity_name) DO UPDATE SET body=excluded.body,source=excluded.source,version=excluded.version,installed_at=CURRENT_TIMESTAMP", params![name,skill.body,skill.source,skill.version])?; tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,'description',?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP", params![normalize(&skill.entity_name),skill.description])?; Ok(()) })
    }
    fn remove_skills(&self, entity_names: Vec<String>) -> ApiResult<()> {
        self.delete_entities(entity_names)
    }
}

impl SnapshotStore for SqliteStore {
    fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot> {
        let graph = if scope.is_empty() {
            self.read_graph_full()?
        } else {
            self.read_graph_scoped(scope, rationale)?
        };
        Ok(Snapshot {
            api_version: crate::api::v2::API_VERSION,
            format_version: crate::api::v2::SNAPSHOT_FORMAT_VERSION,
            source_backend: "sqlite".into(),
            source_schema_version: SCHEMA_VERSION as u32,
            graph,
        })
    }
    fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport> {
        if snapshot.api_version != crate::api::v2::API_VERSION
            || snapshot.format_version != crate::api::v2::SNAPSHOT_FORMAT_VERSION
        {
            return Err(ApiError::Invalid("unsupported snapshot version".into()));
        }
        self.write(|tx| { let mut report = ImportReport::default(); for entity in snapshot.graph.entities { let name=normalize(&entity.name); let inserted=tx.execute("INSERT OR IGNORE INTO asobi_entities(name,entity_type) VALUES (?,?)", params![name,entity.entity_type])?; if inserted==1 {report.entities_created+=1;} for obs in entity.observations { tx.execute("INSERT INTO asobi_observations(entity_name,content) VALUES (?,?)", params![normalize(&entity.name),obs])?; report.observations_added+=1; } for (key,value) in entity.truths { tx.execute("INSERT INTO asobi_truths(entity_name,key,value) VALUES (?,?,?) ON CONFLICT(entity_name,key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP", params![normalize(&entity.name),key,value])?; report.truths_updated+=1; } } for rel in snapshot.graph.relations { tx.execute("INSERT OR REPLACE INTO asobi_relations(from_entity,to_entity,relation_type) VALUES (?,?,?)", params![normalize(&rel.from),normalize(&rel.to),rel.relation_type])?; report.relations_added+=1; } Ok(report) })
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
    fn reset(&self) -> ApiResult<()> {
        self.write(|tx| { tx.execute_batch("DELETE FROM asobi_relations; DELETE FROM asobi_truth_history; DELETE FROM asobi_truths; DELETE FROM asobi_observations; DELETE FROM asobi_skills; DELETE FROM asobi_entities;")?; Ok(()) })
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
