use asobi::api::{
    BackupRequest, BackupStore, GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, SearchQuery,
    SearchStore, SnapshotStore, TaskStore,
};
use asobi::model::{EntityInput, RelationInput};
use asobi::storage::SqliteStore;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

fn store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().unwrap();
    let db = dir.path().join("contract.db");
    let store = SqliteStore::open_at(&db).unwrap();
    (dir, store)
}

#[test]
fn sqlite_implements_the_v2_contract() {
    let (_dir, store) = store();
    let capabilities = store.capabilities().unwrap();
    assert_eq!(capabilities.backend, "sqlite");
    assert_eq!(capabilities.keyword_search_kind, "fts5");
    assert!(capabilities.multi_process);
    assert!(capabilities.physical_backup);
}

#[test]
fn graph_truth_search_and_task_claim_are_atomic_surfaces() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "project:asobi".into(),
            entity_type: "project".into(),
            observations: vec!["SQLite FTS5 supports concurrent agent recall".into()],
        }])
        .unwrap();
    store
        .create_entities(vec![EntityInput {
            name: "asobi:task-1".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();
    store
        .create_relations(vec![RelationInput {
            from: "asobi:task-1".into(),
            to: "project:asobi".into(),
            relation_type: "part_of".into(),
        }])
        .unwrap();
    store
        .truth_upsert("asobi:task-1", "status", "READY_TO_DISPATCH")
        .unwrap();
    store
        .truth_upsert("project:asobi", "status", "ACTIVE")
        .unwrap();

    let graph = store
        .search_nodes(SearchQuery {
            query: "concurrent recall".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    assert_eq!(graph.entities[0].name, "project:asobi");

    let filtered = store
        .search_nodes(SearchQuery {
            query: "concurrent".into(),
            limit: 10,
            filters: vec![("status".into(), "ACTIVE".into())],
        })
        .unwrap();
    assert_eq!(filtered.entities.len(), 1);
    assert_eq!(filtered.entities[0].name, "project:asobi");

    assert_eq!(
        store.claim_next("agent-a").unwrap().as_deref(),
        Some("asobi:task-1")
    );
    assert_eq!(store.claim_next("agent-b").unwrap(), None);
}

#[test]
fn graph_and_search_keep_observations_lazy() {
    let (_dir, store) = store();
    store
        .create_entities(vec![asobi::model::EntityInput {
            name: "lean-read".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }])
        .unwrap();
    store
        .add_observations(
            vec![asobi::model::ObservationInput {
                entity_name: "lean-read".into(),
                contents: vec!["heavy observation".into()],
            }],
            200,
        )
        .unwrap();

    let lean = store.read_graph().unwrap();
    let entity = &lean.entities[0];
    assert_eq!(entity.observation_count, 1);
    assert!(entity.observations.is_empty());
    assert!(entity.observations_detailed.is_none());
    let lean_json = serde_json::to_value(&lean).unwrap();
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("body")
    );
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("observations")
    );
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("observationsDetailed")
    );

    let search = store
        .search_nodes(SearchQuery {
            query: "heavy observation".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    let entity = &search.entities[0];
    assert_eq!(entity.observation_count, 1);
    assert!(entity.observations.is_empty());
    assert!(entity.observations_detailed.is_none());

    let full = store
        .open_nodes(OpenNodes {
            observation_limit: 0,
            names: vec!["lean-read".into()],
            with_ids: true,
            expand: vec![],
        })
        .unwrap();
    let entity = &full.entities[0];
    assert_eq!(entity.observations, vec!["heavy observation"]);
    assert_eq!(entity.observations_detailed.as_ref().unwrap().len(), 1);

    let exported = store.read_graph_full().unwrap();
    assert_eq!(exported.entities[0].observations, vec!["heavy observation"]);
}

#[test]
fn purge_is_preview_first_and_rejects_durable_types() {
    let (dir, store) = store();
    store
        .create_entities(vec![
            EntityInput {
                name: "project:session".into(),
                entity_type: "session".into(),
                observations: vec!["old session note".into()],
            },
            EntityInput {
                name: "project:task".into(),
                entity_type: "task".into(),
                observations: vec!["old task note".into()],
            },
            EntityInput {
                name: "project:concept".into(),
                entity_type: "concept".into(),
                observations: vec!["durable note".into()],
            },
        ])
        .unwrap();
    store
        .truth_upsert("project:session", "status", "DONE")
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .unwrap();

    let db = dir.path().join("contract.db");
    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        "UPDATE asobi_entities SET created_at = datetime('now', '-90 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-90 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-90 days');",
    )
    .unwrap();
    drop(conn);

    let request = PurgeRequest {
        include_truth_history: false,
        entity_types: vec!["session".into(), "task".into()],
        statuses: vec!["DONE".into()],
        older_than_days: 30,
        apply: false,
    };
    let preview = store.purge(request.clone()).unwrap();
    assert!(preview.dry_run);
    assert_eq!(preview.deleted, 0);
    assert_eq!(preview.candidates.len(), 2);
    assert!(
        store
            .open_nodes(OpenNodes {
                observation_limit: 0,
                names: vec!["project:task".into()],
                ..Default::default()
            })
            .unwrap()
            .entities
            .len()
            == 1
    );

    let applied = store
        .purge(PurgeRequest {
            include_truth_history: false,
            apply: true,
            ..request
        })
        .unwrap();
    assert!(!applied.dry_run);
    assert_eq!(applied.deleted, 2);
    assert!(
        store
            .open_nodes(OpenNodes {
                observation_limit: 0,
                names: vec!["project:task".into()],
                ..Default::default()
            })
            .unwrap()
            .entities
            .is_empty()
    );
    assert!(
        store
            .search_nodes(SearchQuery {
                query: "old task note".into(),
                limit: 10,
                filters: vec![],
            })
            .unwrap()
            .entities
            .is_empty()
    );
    assert_eq!(store.read_graph().unwrap().entities.len(), 1);

    let error = store
        .purge(PurgeRequest {
            include_truth_history: false,
            entity_types: vec!["concept".into()],
            statuses: vec!["DONE".into()],
            older_than_days: 30,
            apply: true,
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("restricted to operational entity types")
    );

    let error = store
        .purge(PurgeRequest {
            include_truth_history: false,
            entity_types: vec!["task".into()],
            statuses: vec!["IN_PROGRESS".into()],
            older_than_days: 30,
            apply: false,
        })
        .unwrap_err();
    assert!(error.to_string().contains("only accepts terminal statuses"));
}

#[test]
fn snapshot_and_physical_backup_are_supported() {
    let (dir, live_store) = store();
    live_store
        .create_entities(vec![EntityInput {
            name: "snapshot:test".into(),
            entity_type: "concept".into(),
            observations: vec!["portable graph state".into()],
        }])
        .unwrap();
    let snapshot = live_store.export_snapshot(&[], false).unwrap();
    assert_eq!(snapshot.source_backend, "sqlite");
    assert_eq!(snapshot.graph.entities.len(), 1);

    let backup = dir.path().join("backup.db");
    let receipt = live_store
        .backup(BackupRequest {
            destination: backup.clone(),
            keep: 1,
        })
        .unwrap();
    assert_eq!(receipt.path, backup);
    assert!(backup.exists());
}

#[test]
fn managed_backup_retention_prunes_old_snapshots() {
    let (dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "retention:test".into(),
            entity_type: "concept".into(),
            observations: vec!["retention data".into()],
        }])
        .unwrap();

    for _ in 0..3 {
        store
            .backup(BackupRequest {
                destination: PathBuf::new(),
                keep: 2,
            })
            .unwrap();
    }

    let backups = fs::read_dir(dir.path().join("backups"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("asobi-"))
        .count();
    assert_eq!(backups, 2);
}

#[test]
fn restore_rejects_non_asobi_sqlite_and_removes_sidecars() {
    let (dir, live_store) = store();
    live_store
        .create_entities(vec![EntityInput {
            name: "restore:test".into(),
            entity_type: "concept".into(),
            observations: vec!["restore data".into()],
        }])
        .unwrap();

    let source = dir.path().join("source.db");
    live_store
        .backup(BackupRequest {
            destination: source.clone(),
            keep: 1,
        })
        .unwrap();
    let live = dir.path().join("contract.db");
    fs::write(format!("{}-wal", live.display()), b"stale wal").unwrap();
    fs::write(format!("{}-shm", live.display()), b"stale shm").unwrap();
    live_store.restore(source, true).unwrap();
    assert!(!PathBuf::from(format!("{}-wal", live.display())).exists());
    assert!(!PathBuf::from(format!("{}-shm", live.display())).exists());

    let invalid_source = dir.path().join("invalid.db");
    let invalid = Connection::open(&invalid_source).unwrap();
    invalid
        .execute("CREATE TABLE unrelated (value TEXT)", [])
        .unwrap();
    drop(invalid);
    let (_invalid_dir, invalid_store) = store();
    let error = invalid_store.restore(invalid_source, true).unwrap_err();
    assert!(error.to_string().contains("not an Asobi SQLite database"));
}

// storage-boundary: provider-test -- this test names the libSQL/Turso-era
// schema on purpose, to verify the SQLite provider cleans up after it.
#[test]
fn opening_a_pre_v5_database_drops_superseded_tables_and_enables_incremental_vacuum() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");

    // Reproduce a database that has existed since before the 0.6 rusqlite
    // rewrite: the original `mcp_*` schema plus the libSQL/Turso-era
    // `chunks`/`topics` vector schema, both superseded and left in place by
    // every migration that came after them. `idx_chunks_vector` is the real
    // regression case -- it is an expression index over `libsql_vector_idx`,
    // a function only the libSQL fork registers. Ordinary `CREATE INDEX`
    // evaluates the expression up front, so this rusqlite build (no
    // `functions` feature, no libSQL) can't build that fixture the normal
    // way; `writable_schema` plants the same catalog row libSQL would have
    // written without evaluating it, matching what a real such database
    // looks like to a plain-SQLite reader. The store's own connection below
    // never registers that function either, so the migration must drop
    // `chunks` before VACUUM ever has to rebuild the index.
    {
        let legacy = Connection::open(&db_path).unwrap();
        legacy
            .execute_batch(
                "CREATE TABLE mcp_entities (name TEXT PRIMARY KEY);
                 CREATE TABLE chunks (id INTEGER PRIMARY KEY, embedding BLOB);
                 CREATE TABLE topics (id INTEGER PRIMARY KEY, title TEXT);
                 CREATE TABLE sessions (id INTEGER PRIMARY KEY);
                 INSERT INTO mcp_entities(name) VALUES ('leftover');
                 PRAGMA user_version = 4;
                 PRAGMA writable_schema = ON;
                 INSERT INTO sqlite_master(type, name, tbl_name, rootpage, sql) VALUES (
                     'index', 'idx_chunks_vector', 'chunks', 0,
                     'CREATE INDEX idx_chunks_vector ON chunks(libsql_vector_idx(embedding, ''metric=cosine''))'
                 );
                 PRAGMA writable_schema = RESET;",
            )
            .unwrap();
    }
    let store = SqliteStore::open_at(&db_path).unwrap();
    // The current-generation graph works normally post-migration.
    store
        .create_entities(vec![EntityInput {
            name: "post-migration".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }])
        .unwrap();
    drop(store);

    let conn = Connection::open(&db_path).unwrap();
    let table_count: i64 = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table'
             AND name IN ('mcp_entities', 'chunks', 'topics', 'sessions')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 0, "superseded tables should be dropped");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_version, 6);
    let auto_vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        auto_vacuum, 2,
        "database should switch to incremental auto-vacuum"
    );

    // Re-opening an already-migrated database is a no-op: no error, and the
    // migration path does not run again.
    let reopened = SqliteStore::open_at(&db_path).unwrap();
    assert_eq!(reopened.stats().unwrap().entities, 1);
}

#[test]
fn applied_purge_reclaims_space_via_incremental_vacuum() {
    let (dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec!["done".into()],
        }])
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .unwrap();

    let db = dir.path().join("contract.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "UPDATE asobi_entities SET created_at = datetime('now', '-90 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-90 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-90 days');",
    )
    .unwrap();
    drop(conn);

    let report = store
        .purge(PurgeRequest {
            include_truth_history: false,
            entity_types: vec!["task".into()],
            statuses: vec!["DONE".into()],
            older_than_days: 30,
            apply: true,
        })
        .unwrap();
    assert_eq!(report.deleted, 1);

    // `incremental_vacuum` after an applied purge must not error even
    // though this store was opened fresh (auto_vacuum already INCREMENTAL).
    let conn = Connection::open(&db).unwrap();
    let auto_vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(auto_vacuum, 2);
}

/// A scoped export is the documented way to hand an epic to another agent, and
/// until 0.7 it carried only current state -- so the receiving agent could not
/// tell a fact that was always true from one corrected an hour earlier. The
/// change trail has to survive the round trip.
#[test]
fn scoped_export_carries_the_truth_change_trail() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:epic".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();
    store.truth_upsert("proj:epic", "status", "REVIEW").unwrap();
    store.truth_upsert("proj:epic", "status", "DONE").unwrap();

    let exported = store
        .export_snapshot(&["proj:epic".to_string()], false)
        .unwrap();
    let history = &exported.truth_history;
    assert_eq!(history.len(), 1, "expected history for the exported entity");
    assert_eq!(history[0].entity_name, "proj:epic");
    assert_eq!(history[0].versions.len(), 1);
    assert_eq!(history[0].versions[0].value, "REVIEW");

    // Round-trip into a clean graph: the superseded value comes back.
    store.reset().unwrap();
    store.import_snapshot(exported.clone()).unwrap();
    let restored = store.truth_history("proj:epic", None).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].value, "REVIEW");

    // Re-importing the same snapshot must not duplicate the trail.
    store.import_snapshot(exported).unwrap();
    assert_eq!(store.truth_history("proj:epic", None).unwrap().len(), 1);
}

/// `show` is the only eager read, and it was unbounded: loading a session at
/// the 200-observation cap cost tens of thousands of tokens to answer "where
/// was I". Context is the scarce resource, so it returns the current end of the
/// trail by default — while still reporting the true total, since a caller that
/// cannot tell what it is missing is worse off than one reading everything.
#[test]
fn show_returns_recent_observations_and_the_true_total() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:session".into(),
            entity_type: "session".into(),
            observations: vec![],
        }])
        .unwrap();
    for i in 0..50 {
        store
            .add_observations(
                vec![asobi::model::ObservationInput {
                    entity_name: "proj:session".into(),
                    contents: vec![format!("note {i}")],
                }],
                200,
            )
            .unwrap();
    }

    let limited = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 10,
            ..Default::default()
        })
        .unwrap();
    let entity = &limited.entities[0];
    assert_eq!(entity.observations.len(), 10, "should return the limit");
    assert_eq!(entity.observation_count, 50, "count is the true total");
    // Newest kept, and still in written order so a truncated trail reads forward.
    assert_eq!(entity.observations.first().unwrap(), "note 40");
    assert_eq!(entity.observations.last().unwrap(), "note 49");

    // 0 means the whole trail, which is what export relies on.
    let full = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(full.entities[0].observations.len(), 50);
    assert_eq!(
        store.read_graph_full().unwrap().entities[0]
            .observations
            .len(),
        50
    );
}

/// Truth history is the one store with no bound of its own, so purge has to be
/// able to find it. Superseded versions are safe to drop by construction: the
/// current value lives in a different table and is never touched.
#[test]
fn purge_finds_expired_truth_versions_without_deleting_them_by_default() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:session".into(),
            entity_type: "session".into(),
            observations: vec![],
        }])
        .unwrap();
    for value in ["first", "second", "third"] {
        store.truth_upsert("proj:session", "next", value).unwrap();
    }

    let request = |apply: bool, history: bool| PurgeRequest {
        entity_types: vec!["session".into()],
        statuses: vec!["DONE".into()],
        older_than_days: 0,
        apply,
        include_truth_history: history,
    };

    let preview = store.purge(request(false, false)).unwrap();
    let found: usize = preview
        .expired_truth_versions
        .iter()
        .map(|g| g.versions)
        .sum();
    assert_eq!(found, 2, "two superseded values behind the current one");
    assert_eq!(
        preview.deleted_truth_versions, 0,
        "a preview deletes nothing"
    );

    // Applying without --history leaves the trail alone.
    store.purge(request(true, false)).unwrap();
    assert_eq!(store.truth_history("proj:session", None).unwrap().len(), 2);

    let applied = store.purge(request(true, true)).unwrap();
    assert_eq!(applied.deleted_truth_versions, 2);
    assert!(
        store
            .truth_history("proj:session", None)
            .unwrap()
            .is_empty()
    );
    // The current value survives: only superseded versions are in scope.
    let current = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(current.entities[0].truths.get("next").unwrap(), "third");
}
