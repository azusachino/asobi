use asobi::api::v1::{Backend, DocumentChunk, DocumentStore, MaintenanceStore};
use asobi::backend::TursoBackend;

fn assert_backend_contract<B: Backend>() {}

#[test]
fn turso_satisfies_the_versioned_backend_contract() {
    assert_backend_contract::<TursoBackend>();
}

#[tokio::test]
async fn turso_reports_unimplemented_optional_capabilities_explicitly() {
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
    assert!(!capabilities.vectors);
    assert!(!capabilities.logical_snapshots);

    let error = backend
        .insert_chunks(vec![DocumentChunk {
            id: "chunk".to_string(),
            topic_id: "topic".to_string(),
            chunk_idx: 0,
            text: "text".to_string(),
            source: "source".to_string(),
            embedding: vec![0.0; 384],
        }])
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        asobi::api::v1::ApiError::Unsupported("vectors")
    ));

    let health = backend.health().await.unwrap();
    assert!(health.reachable);
}
