use asobi::{db, model};
use std::fs;
use tempfile::tempdir;

async fn test_conn() -> (tempfile::TempDir, libsql::Connection) {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("edge-cases.db");
    unsafe {
        std::env::set_var(db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }
    let (_db, conn) = db::init_db().await.unwrap();
    (dir, conn)
}

#[tokio::test]
async fn graph_crud_handles_edges() {
    let (_dir, conn) = test_conn().await;

    db::create_entities(
        &conn,
        vec![
            model::EntityInput {
                name: "alpha".into(),
                entity_type: "project".into(),
                observations: vec!["running async tasks".into()],
            },
            model::EntityInput {
                name: "beta".into(),
                entity_type: "concept".into(),
                observations: vec!["scheduler queue".into()],
            },
        ],
    )
    .await
    .unwrap();

    db::create_relations(
        &conn,
        vec![model::RelationInput {
            from: "alpha".into(),
            to: "beta".into(),
            relation_type: "uses".into(),
        }],
    )
    .await
    .unwrap();

    // "missing" should be safely ignored by open_nodes
    let graph = db::open_nodes(&conn, vec!["alpha".into(), "missing".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "alpha");
    assert_eq!(graph.relations.len(), 1); // 1-hop expansion brings in the relation

    let hits = db::search_nodes(&conn, "run").await.unwrap();
    assert_eq!(hits.entities.len(), 2); // 'alpha' matched FTS, 'beta' brought in by 1-hop relation expansion!
    assert!(hits.entities.iter().any(|e| e.name == "alpha"));

    // creating a relation to a missing entity should fail foreign key constraints
    let bad_relation = db::create_relations(
        &conn,
        vec![model::RelationInput {
            from: "alpha".into(),
            to: "missing".into(),
            relation_type: "uses".into(),
        }],
    )
    .await;
    assert!(bad_relation.is_err());

    db::delete_entities(&conn, vec!["beta".into()])
        .await
        .unwrap();

    let graph = db::open_nodes(&conn, vec!["alpha".into(), "beta".into()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert!(graph.relations.is_empty());

    // Deleting a non-existent entity should be a no-op, no panic
    db::delete_entities(&conn, vec!["missing".into()])
        .await
        .unwrap();
}

#[tokio::test]
async fn graph_accepts_irregular_text_without_sql_injection() {
    let (_dir, conn) = test_conn().await;
    let raw_name = "node-日本語-'; DROP TABLE mcp_entities; --";
    let normalized_name = asobi::normalize::normalize_key(raw_name);
    let odd_observation = "日本語 русский عربى control:\u{0007}\nquote:' double:\" percent:%";
    let large_observation = "large-observation ".repeat(16_384);

    db::create_entities(
        &conn,
        vec![model::EntityInput {
            name: raw_name.into(),
            entity_type: "project'; DROP TABLE mcp_observations; --".into(),
            observations: vec![odd_observation.into(), large_observation.clone()],
        }],
    )
    .await
    .unwrap();

    // Duplicate create is a complete no-op, including supplied observations.
    db::create_entities(
        &conn,
        vec![model::EntityInput {
            name: raw_name.into(),
            entity_type: "ignored".into(),
            observations: vec!["second insert observation".into()],
        }],
    )
    .await
    .unwrap();

    let opened = db::open_nodes(&conn, vec![raw_name.into()]).await.unwrap();
    assert_eq!(opened.entities.len(), 1);
    assert_eq!(opened.entities[0].name, normalized_name);
    // The entity type is NOT part of the key namespace, so it remains un-normalized raw string
    assert_eq!(
        opened.entities[0].entity_type,
        "project'; DROP TABLE mcp_observations; --"
    );
    assert_eq!(opened.entities[0].observations.len(), 2);
    assert!(opened.entities[0].observations.contains(&large_observation));

    let unicode_hits = db::search_nodes(&conn, "日本語").await.unwrap();
    // 1-hop relation expansion applies, but here it's isolated
    assert_eq!(unicode_hits.entities.len(), 1);

    // SQL injection text in observations should be safely searchable without syntax errors
    let injection_hits = db::search_nodes(&conn, "drop").await.unwrap();
    assert_eq!(injection_hits.entities.len(), 1);

    db::create_entities(
        &conn,
        vec![model::EntityInput {
            name: "safe-target".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }],
    )
    .await
    .unwrap();

    db::create_relations(
        &conn,
        vec![model::RelationInput {
            from: raw_name.into(),
            to: "safe-target".into(),
            relation_type: "relates'; DELETE FROM mcp_relations; --".into(),
        }],
    )
    .await
    .unwrap();

    let related = db::open_nodes(&conn, vec![raw_name.into(), "safe-target".into()])
        .await
        .unwrap();
    assert_eq!(related.relations.len(), 1);
    assert_eq!(
        related.relations[0].relation_type,
        "relates'; DELETE FROM mcp_relations; --"
    );

    let all = db::read_graph(&conn).await.unwrap();
    assert_eq!(all.entities.len(), 2);
}

#[tokio::test]
async fn graph_deletes_exact_observation_only() {
    let (_dir, conn) = test_conn().await;
    db::create_entities(
        &conn,
        vec![model::EntityInput {
            name: "exact".into(),
            entity_type: "project".into(),
            observations: vec!["same prefix".into(), "same prefix extended".into()],
        }],
    )
    .await
    .unwrap();

    db::delete_observations(
        &conn,
        vec![model::ObservationDeletion {
            entity_name: "exact".into(),
            observations: vec!["same prefix".into()],
        }],
    )
    .await
    .unwrap();

    let opened = db::open_nodes(&conn, vec!["exact".into()]).await.unwrap();
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
        std::env::set_var(db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }

    let result = db::init_db().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn graph_handles_malicious_payloads_gracefully() {
    let (_dir, conn) = test_conn().await;

    let payloads = [
        "<script>alert('XSS')</script>", // XSS
        "$(rm -rf /)",                   // Command Injection
        "../../../etc/passwd",           // Path Traversal
        "bell\x07byte\n\rtest",          // Control chars
        "{\"injected\": true}",          // JSON injection
    ];

    for (i, payload) in payloads.iter().enumerate() {
        let raw_name = format!("hacker-{}", i);

        // Use the malicious payload as the entity type and the observation.
        // Use a safe name so we can reliably fetch it.
        db::create_entities(
            &conn,
            vec![model::EntityInput {
                name: raw_name.clone(),
                entity_type: payload.to_string(),
                observations: vec![payload.to_string()],
            }],
        )
        .await
        .unwrap();

        let opened = db::open_nodes(&conn, vec![raw_name.clone()]).await.unwrap();

        assert_eq!(opened.entities.len(), 1);
        // The entity_type should retain the exact malicious payload
        assert_eq!(opened.entities[0].entity_type, *payload);
        // The observation should retain the exact malicious payload
        assert_eq!(opened.entities[0].observations[0], *payload);

        // Create a relation using the malicious payload as the relation type
        db::create_entities(
            &conn,
            vec![model::EntityInput {
                name: "safe-target".to_string(),
                entity_type: "safe".to_string(),
                observations: vec![],
            }],
        )
        .await
        .unwrap();

        db::create_relations(
            &conn,
            vec![model::RelationInput {
                from: raw_name.clone(),
                to: "safe-target".to_string(),
                relation_type: payload.to_string(),
            }],
        )
        .await
        .unwrap();

        let related = db::open_nodes(&conn, vec![raw_name.clone(), "safe-target".to_string()])
            .await
            .unwrap();

        // Find the relation we just added
        let rel = related
            .relations
            .iter()
            .find(|r| r.from == raw_name && r.to == "safe-target")
            .unwrap();
        // The relation type should retain the exact malicious payload
        assert_eq!(rel.relation_type, *payload);
    }
}
