use rosemary::{db, mcp};
use std::fs;
use tempfile::tempdir;

async fn test_conn() -> (tempfile::TempDir, libsql::Connection) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("edge-cases.db");
    unsafe {
        std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
    }
    let (_db, conn) = db::init_db().await.unwrap();
    (dir, conn)
}

#[tokio::test]
async fn graph_crud_handles_edges() {
    let (_dir, conn) = test_conn().await;

    db::mcp_create_entities(
        &conn,
        vec![
            mcp::EntityInput {
                name: "alpha".into(),
                entity_type: "project".into(),
                observations: vec!["running async tasks".into()],
            },
            mcp::EntityInput {
                name: "beta".into(),
                entity_type: "concept".into(),
                observations: vec!["scheduler queue".into()],
            },
        ],
    )
    .await
    .unwrap();

    db::mcp_create_relations(
        &conn,
        vec![mcp::RelationInput {
            from: "alpha".into(),
            to: "beta".into(),
            relation_type: "uses".into(),
        }],
    )
    .await
    .unwrap();

    let graph = db::mcp_open_nodes(&conn, vec!["alpha".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "alpha");
    assert_eq!(graph.relations.len(), 1);

    let hits = db::mcp_search_nodes(&conn, "run").await.unwrap();
    assert_eq!(hits.entities.len(), 2);
    assert_eq!(hits.entities[0].name, "alpha");

    // "missing" is indeed missing, so this should not create a relation
    let bad_relation = db::mcp_create_relations(
        &conn,
        vec![mcp::RelationInput {
            from: "alpha".into(),
            to: "missing-node".into(),
            relation_type: "uses".into(),
        }],
    )
    .await;
    assert!(bad_relation.is_err());

    db::mcp_delete_entities(&conn, vec!["beta".into()])
        .await
        .unwrap();
    let graph = db::mcp_open_nodes(&conn, vec!["alpha".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert!(graph.relations.is_empty());
}

#[tokio::test]
async fn graph_accepts_irregular_text_without_sql_injection() {
    let (_dir, conn) = test_conn().await;
    let raw_name = "node-日本語-'; DROP TABLE mcp_entities; --";
    let normalized_name = rosemary::normalize::normalize_key(raw_name);

    db::mcp_create_entities(
        &conn,
        vec![mcp::EntityInput {
            name: raw_name.into(),
            entity_type: "project-type".into(),
            observations: vec!["some obs".into()],
        }],
    )
    .await
    .unwrap();

    let opened = db::mcp_open_nodes(&conn, vec![normalized_name.clone()])
        .await
        .unwrap();
    assert_eq!(opened.entities.len(), 1);
    assert_eq!(opened.entities[0].name, normalized_name);

    db::mcp_create_entities(
        &conn,
        vec![mcp::EntityInput {
            name: "safe-target".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }],
    )
    .await
    .unwrap();

    db::mcp_create_relations(
        &conn,
        vec![mcp::RelationInput {
            from: normalized_name.clone(),
            to: "safe-target".into(),
            relation_type: "relates".into(),
        }],
    )
    .await
    .unwrap();

    let related = db::mcp_open_nodes(&conn, vec![normalized_name.clone(), "safe-target".into()])
        .await
        .unwrap();
    assert_eq!(related.relations.len(), 1);
    assert_eq!(related.relations[0].from, normalized_name);
}

#[tokio::test]
async fn graph_deletes_exact_observation_only() {
    let (_dir, conn) = test_conn().await;
    db::mcp_create_entities(
        &conn,
        vec![mcp::EntityInput {
            name: "exact".into(),
            entity_type: "project".into(),
            observations: vec!["same prefix".into(), "same prefix extended".into()],
        }],
    )
    .await
    .unwrap();

    db::mcp_delete_observations(
        &conn,
        vec![mcp::ObservationDeletion {
            entity_name: "exact".into(),
            observations: vec!["same prefix".into()],
        }],
    )
    .await
    .unwrap();

    let opened = db::mcp_open_nodes(&conn, vec!["exact".into()])
        .await
        .unwrap();
    assert_eq!(
        opened.entities[0].observations,
        vec!["same prefix extended".to_string()]
    );
}

#[tokio::test]
async fn corrupted_database_returns_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("corrupt.db");
    fs::write(&db_path, b"this is not a sqlite database").unwrap();
    unsafe {
        std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
    }

    let result = db::init_db().await;
    assert!(result.is_err());
}
