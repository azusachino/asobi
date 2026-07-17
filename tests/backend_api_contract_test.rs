use asobi::api::{
    BackupRequest, BackupStore, GraphStore, MaintenanceStore, SearchQuery, SearchStore,
    SnapshotStore, TaskStore,
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
