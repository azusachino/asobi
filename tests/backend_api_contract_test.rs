use asobi::api::v1::{
    Backend, DocumentChunk, DocumentStore, GraphStore, MaintenanceStore, SearchQuery, SearchStore,
};
use asobi::backend::{LibsqlBackend, TursoBackend};

fn assert_backend_contract<B: Backend>() {}

#[test]
fn turso_satisfies_the_versioned_backend_contract() {
    assert_backend_contract::<TursoBackend>();
}

#[test]
fn libsql_satisfies_the_versioned_backend_contract() {
    assert_backend_contract::<LibsqlBackend>();
}

#[tokio::test]
async fn turso_reports_optional_capabilities_explicitly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("api-contract.db");
    unsafe {
        std::env::set_var(
            asobi::backend::turso::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = TursoBackend::open().await.unwrap();
    let capabilities = backend.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "turso");
    assert!(capabilities.keyword_search);
    assert_eq!(capabilities.documents, cfg!(feature = "documents"));
    assert_eq!(capabilities.vectors, cfg!(feature = "documents"));
    assert!(!capabilities.logical_snapshots);

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
            asobi::backend::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlBackend::open().await.unwrap();
    let capabilities = backend.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "libsql");
    assert!(capabilities.keyword_search);
    assert_eq!(capabilities.documents, cfg!(feature = "documents"));
    assert_eq!(capabilities.vectors, cfg!(feature = "documents"));
    assert!(!capabilities.logical_snapshots);

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
            asobi::backend::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlBackend::open().await.unwrap();
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
            asobi::backend::libsql::constant::ENV_DATABASE_URL,
            db_path.to_str().unwrap(),
        );
    }

    let backend = LibsqlBackend::open().await.unwrap();
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
