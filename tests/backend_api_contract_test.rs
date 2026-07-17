use asobi::api::{
    BackupRequest, BackupStore, GraphStore, MaintenanceStore, SearchQuery, SearchStore,
    SnapshotStore, TaskStore,
};
use asobi::model::{EntityInput, RelationInput};
use asobi::storage::SqliteStore;
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

    let graph = store
        .search_nodes(SearchQuery {
            query: "concurrent recall".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    assert_eq!(graph.entities[0].name, "project:asobi");

    assert_eq!(
        store.claim_next("agent-a").unwrap().as_deref(),
        Some("asobi:task-1")
    );
    assert_eq!(store.claim_next("agent-b").unwrap(), None);
}

#[test]
fn snapshot_and_physical_backup_are_supported() {
    let (dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "snapshot:test".into(),
            entity_type: "concept".into(),
            observations: vec!["portable graph state".into()],
        }])
        .unwrap();
    let snapshot = store.export_snapshot(&[], false).unwrap();
    assert_eq!(snapshot.source_backend, "sqlite");
    assert_eq!(snapshot.graph.entities.len(), 1);

    let backup = dir.path().join("backup.db");
    let receipt = store
        .backup(BackupRequest {
            destination: backup.clone(),
            keep: 1,
        })
        .unwrap();
    assert_eq!(receipt.path, backup);
    assert!(backup.exists());
}
