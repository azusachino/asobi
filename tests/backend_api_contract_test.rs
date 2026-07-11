use asobi::api::v1::{
    BackupStore, DocumentChunk, DocumentMaintenanceStore, DocumentStore, GraphStore,
    MaintenanceStore, SNAPSHOT_FORMAT_VERSION, SearchQuery, SearchStore, SkillStore, Snapshot,
    SnapshotStore,
};
use asobi::application::AsobiRuntime;
#[cfg(feature = "turso-experimental")]
use asobi::storage::TursoStore;
use asobi::storage::{LibsqlStore, Storage};

fn assert_backend_contract<B: GraphStore + SearchStore + MaintenanceStore>() {}

#[tokio::test]
async fn application_runtime_exports_backend_neutral_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var(
            asobi::storage::libsql::db::ENV_DATABASE_URL,
            dir.path().join("runtime.db").to_str().unwrap(),
        );
    }
    let runtime = AsobiRuntime::open_default().await.unwrap();
    runtime
        .storage()
        .create_entities(vec![asobi::model::EntityInput {
            name: "runtime".to_string(),
            entity_type: "task".to_string(),
            observations: vec!["composed".to_string()],
        }])
        .await
        .unwrap();
    let snapshot = runtime.export_snapshot(&[], false).await.unwrap();
    assert_eq!(snapshot.source_backend, "libsql");
    assert_eq!(snapshot.graph.entities[0].name, "runtime");
    runtime.storage().reset().await.unwrap();
    let report = runtime.import_snapshot(snapshot).await.unwrap();
    assert_eq!(report.entities_created, 1);
    assert_eq!(
        runtime
            .storage()
            .read_graph_full()
            .await
            .unwrap()
            .entities
            .len(),
        1
    );
}

#[allow(dead_code)]
fn assert_extended_contract_shape<
    B: SkillStore + SnapshotStore + BackupStore + DocumentMaintenanceStore,
>() {
}

#[test]
fn logical_snapshot_round_trips_without_driver_state() {
    let snapshot = Snapshot {
        api_version: asobi::api::v1::API_VERSION,
        format_version: SNAPSHOT_FORMAT_VERSION,
        source_backend: "libsql".to_string(),
        source_schema_version: 1,
        graph: asobi::model::Graph {
            entities: Vec::new(),
            relations: Vec::new(),
        },
    };
    let encoded = serde_json::to_string(&snapshot).unwrap();
    let decoded: Snapshot = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.api_version, 1);
    assert_eq!(decoded.format_version, SNAPSHOT_FORMAT_VERSION);
    assert_eq!(decoded.source_backend, "libsql");
}

#[cfg(feature = "turso-experimental")]
#[test]
fn turso_satisfies_the_versioned_backend_contract() {
    assert_backend_contract::<TursoStore>();
}

#[test]
fn libsql_satisfies_the_versioned_backend_contract() {
    assert_backend_contract::<LibsqlStore>();
}

#[tokio::test]
async fn storage_default_selects_libsql() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("default-storage.db");
    unsafe {
        std::env::set_var(
            asobi::storage::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let storage = Storage::open_default().await.unwrap();
    let capabilities = storage.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "libsql");
}

#[cfg(feature = "turso-experimental")]
#[tokio::test]
async fn turso_reports_optional_capabilities_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("api-contract.db");
    unsafe {
        std::env::set_var(
            asobi::storage::turso::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = TursoStore::open().await.unwrap();
    let capabilities = backend.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "turso");
    // Keyword search is correct (substring scan) even without native FTS.
    assert!(capabilities.keyword_search);
    assert_eq!(capabilities.documents, cfg!(feature = "documents"));
    assert_eq!(capabilities.vectors, cfg!(feature = "documents"));
    assert!(capabilities.logical_snapshots);

    let result = backend
        .insert_chunks(vec![DocumentChunk {
            id: "chunk".to_string(),
            topic_id: "topic".to_string(),
            chunk_idx: 0,
            text: "text".to_string(),
            source: "source".to_string(),
            embedding: vec![0.0; 384],
        }])
        .await;
    if cfg!(feature = "documents") {
        result.unwrap();
    } else {
        assert!(matches!(
            result.unwrap_err(),
            asobi::api::v1::ApiError::Unsupported("vectors")
        ));
    }

    let health = backend.health().await.unwrap();
    assert!(health.reachable);
}

#[tokio::test]
async fn libsql_reports_optional_capabilities_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("api-contract-libsql.db");
    unsafe {
        std::env::set_var(
            asobi::storage::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlStore::open().await.unwrap();
    let capabilities = backend.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "libsql");
    assert!(capabilities.keyword_search);
    assert_eq!(capabilities.documents, cfg!(feature = "documents"));
    assert_eq!(capabilities.vectors, cfg!(feature = "documents"));
    assert!(capabilities.logical_snapshots);

    let result = backend
        .insert_chunks(vec![DocumentChunk {
            id: "chunk".to_string(),
            topic_id: "topic".to_string(),
            chunk_idx: 0,
            text: "text".to_string(),
            source: "source".to_string(),
            embedding: vec![0.0; 384],
        }])
        .await;
    if cfg!(feature = "documents") {
        result.unwrap();
    } else {
        assert!(matches!(
            result.unwrap_err(),
            asobi::api::v1::ApiError::Unsupported("vectors")
        ));
    }

    let health = backend.health().await.unwrap();
    assert!(health.reachable);
}

/// libsql regains real SQLite FTS5 porter stemming, which the turso port had
/// to weaken to token-matching (see `test_search_nodes_stemming` in
/// `src/backend/turso/db.rs`, which only asserts the exact indexed token
/// still matches) — a query stem like "run" must match an indexed inflection
/// like "running".
#[tokio::test]
async fn libsql_search_nodes_stems_across_word_forms() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("libsql-stemming.db");
    unsafe {
        std::env::set_var(
            asobi::storage::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlStore::open().await.unwrap();
    backend
        .create_entities(vec![asobi::model::EntityInput {
            name: "async-patterns".to_string(),
            entity_type: "concept".to_string(),
            observations: vec!["running async tasks efficiently".to_string()],
        }])
        .await
        .unwrap();

    // Query with the bare stem "run" — porter stemming must still match the
    // indexed inflection "running".
    let graph = backend
        .search_nodes(SearchQuery {
            query: "run".to_string(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "async-patterns");
}

/// bm25 ranking must place the entity matching both query terms ahead of the
/// entity matching only one.
#[tokio::test]
async fn libsql_search_nodes_bm25_orders_by_relevance() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("libsql-bm25.db");
    unsafe {
        std::env::set_var(
            asobi::storage::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlStore::open().await.unwrap();
    backend
        .create_entities(vec![
            asobi::model::EntityInput {
                name: "alpha".to_string(),
                entity_type: "project".to_string(),
                observations: vec!["async tokio runtime patterns".to_string()],
            },
            asobi::model::EntityInput {
                name: "beta".to_string(),
                entity_type: "project".to_string(),
                observations: vec!["tokio scheduler".to_string()],
            },
        ])
        .await
        .unwrap();

    let graph = backend
        .search_nodes(SearchQuery {
            query: "async tokio".to_string(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    assert!(!graph.entities.is_empty());
    assert_eq!(graph.entities[0].name, "alpha");
}
// storage-boundary: provider-test
