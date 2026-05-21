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

    let graph = db::mcp_open_nodes(&conn, vec!["alpha".into(), "missing".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "alpha");
    assert!(graph.relations.is_empty());

    let hits = db::mcp_search_nodes(&conn, "run").await.unwrap();
    assert_eq!(hits.entities.len(), 1);
    assert_eq!(hits.entities[0].name, "alpha");

    let bad_relation = db::mcp_create_relations(
        &conn,
        vec![mcp::RelationInput {
            from: "alpha".into(),
            to: "missing".into(),
            relation_type: "uses".into(),
        }],
    )
    .await;
    assert!(bad_relation.is_err());

    db::mcp_delete_entities(&conn, vec!["beta".into()])
        .await
        .unwrap();
    let graph = db::mcp_open_nodes(&conn, vec!["alpha".into(), "beta".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert!(graph.relations.is_empty());

    db::mcp_delete_entities(&conn, vec!["missing".into()])
        .await
        .unwrap();
}

#[tokio::test]
async fn graph_accepts_irregular_text_without_sql_injection() {
    let (_dir, conn) = test_conn().await;
    let suspicious_name = "node-日本語-'; DROP TABLE mcp_entities; --";
    let odd_observation = "日本語 русский عربى control:\u{0007}\nquote:' double:\" percent:%";
    let large_observation = "large-observation ".repeat(16_384);

    db::mcp_create_entities(
        &conn,
        vec![mcp::EntityInput {
            name: suspicious_name.into(),
            entity_type: "project'; DROP TABLE mcp_observations; --".into(),
            observations: vec![odd_observation.into(), large_observation.clone()],
        }],
    )
    .await
    .unwrap();

    // Duplicate create is an entity no-op, but supplied observations are still appended.
    db::mcp_create_entities(
        &conn,
        vec![mcp::EntityInput {
            name: suspicious_name.into(),
            entity_type: "ignored".into(),
            observations: vec!["second insert observation".into()],
        }],
    )
    .await
    .unwrap();

    let opened = db::mcp_open_nodes(&conn, vec![suspicious_name.into()])
        .await
        .unwrap();
    assert_eq!(opened.entities.len(), 1);
    assert_eq!(opened.entities[0].name, suspicious_name);
    assert_eq!(
        opened.entities[0].entity_type,
        "project'; DROP TABLE mcp_observations; --"
    );
    assert_eq!(opened.entities[0].observations.len(), 3);
    assert!(opened.entities[0].observations.contains(&large_observation));

    let unicode_hits = db::mcp_search_nodes(&conn, "日本語").await.unwrap();
    assert_eq!(unicode_hits.entities.len(), 1);

    let injection_hits = db::mcp_search_nodes(&conn, "'; DROP TABLE mcp_entities; --")
        .await
        .unwrap();
    assert_eq!(injection_hits.entities.len(), 1);

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
            from: suspicious_name.into(),
            to: "safe-target".into(),
            relation_type: "relates'; DELETE FROM mcp_relations; --".into(),
        }],
    )
    .await
    .unwrap();

    let related = db::mcp_open_nodes(&conn, vec![suspicious_name.into(), "safe-target".into()])
        .await
        .unwrap();
    assert_eq!(related.relations.len(), 1);
    assert_eq!(
        related.relations[0].relation_type,
        "relates'; DELETE FROM mcp_relations; --"
    );

    let all = db::mcp_read_graph(&conn).await.unwrap();
    assert_eq!(all.entities.len(), 2);
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
