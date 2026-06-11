use rosemary::db::init_db;
use rosemary::mcp::handle_tools_call;
use serde_json::json;
use tempfile::tempdir;

#[tokio::test]
async fn test_create_entities_returns_graph_state() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    unsafe {
        std::env::set_var(rosemary::db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }
    let (_db, conn) = init_db().await.unwrap();
    let conn = &conn;

    let args = json!({
        "entities": [{
            "name": "Project Alpha",
            "entityType": "project",
            "observations": ["initial observation"]
        }]
    });

    let params = json!({
        "name": "create_entities",
        "arguments": args
    });

    let res = handle_tools_call(conn, json!(1), params).await.unwrap();
    let binding = res.result.unwrap();
    let result_content = binding["content"][0]["text"].as_str().unwrap();
    let graph: rosemary::mcp::Graph = serde_json::from_str(result_content).unwrap();

    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "Project-Alpha");
}

#[tokio::test]
async fn test_add_observations_returns_graph_state() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    unsafe {
        std::env::set_var(rosemary::db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }
    let (_db, conn) = init_db().await.unwrap();
    let conn = &conn;

    // Setup entity
    let _ = handle_tools_call(
        conn,
        json!(1),
        json!({
            "name": "create_entities",
            "arguments": {
                "entities": [{ "name": "e1", "entityType": "t1", "observations": ["obs1"] }]
            }
        }),
    )
    .await;

    // Add observation
    let res = handle_tools_call(
        conn,
        json!(2),
        json!({
            "name": "add_observations",
            "arguments": {
                "observations": [{ "entityName": "e1", "contents": ["obs2"] }]
            }
        }),
    )
    .await
    .unwrap();

    let binding = res.result.unwrap();
    let result_content = binding["content"][0]["text"].as_str().unwrap();
    let graph: rosemary::mcp::Graph = serde_json::from_str(result_content).unwrap();

    assert_eq!(graph.entities[0].observations.len(), 2);
    assert!(graph.entities[0].observations.contains(&"obs2".to_string()));
}

#[tokio::test]
async fn test_add_truth_returns_graph_state() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    unsafe {
        std::env::set_var(rosemary::db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }
    let (_db, conn) = init_db().await.unwrap();
    let conn = &conn;

    // 1. Setup entity
    let _ = handle_tools_call(
        conn,
        json!(1),
        json!({
            "name": "create_entities",
            "arguments": {
                "entities": [{ "name": "e1", "entityType": "t1", "observations": [] }]
            }
        }),
    )
    .await;

    // 2. Add truth
    let res = handle_tools_call(
        conn,
        json!(2),
        json!({
            "name": "add_truth",
            "arguments": {
                "entityName": "e1",
                "key": "role",
                "value": "developer"
            }
        }),
    )
    .await
    .unwrap();

    let binding = res.result.unwrap();
    let result_content = binding["content"][0]["text"].as_str().unwrap();
    let graph: rosemary::mcp::Graph = serde_json::from_str(result_content).unwrap();

    assert_eq!(graph.entities.len(), 1);
    let truths = &graph.entities[0].truths;
    assert_eq!(truths.len(), 1);
    assert_eq!(truths.get("role").unwrap(), "developer");
}

#[tokio::test]
async fn test_delete_truth_returns_graph_state() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    unsafe {
        std::env::set_var(rosemary::db::ENV_DATABASE_URL, db_path.to_str().unwrap());
    }
    let (_db, conn) = init_db().await.unwrap();
    let conn = &conn;

    // 1. Setup entity
    let _ = handle_tools_call(
        conn,
        json!(1),
        json!({
            "name": "create_entities",
            "arguments": {
                "entities": [{ "name": "e1", "entityType": "t1", "observations": [] }]
            }
        }),
    )
    .await;

    // 2. Add truth
    let _ = handle_tools_call(
        conn,
        json!(2),
        json!({
            "name": "add_truth",
            "arguments": {
                "entityName": "e1",
                "key": "role",
                "value": "developer"
            }
        }),
    )
    .await;

    // 3. Delete truth
    let res = handle_tools_call(
        conn,
        json!(3),
        json!({
            "name": "delete_truth",
            "arguments": {
                "entityName": "e1",
                "key": "role"
            }
        }),
    )
    .await
    .unwrap();

    let binding = res.result.unwrap();
    let result_content = binding["content"][0]["text"].as_str().unwrap();
    let graph: rosemary::mcp::Graph = serde_json::from_str(result_content).unwrap();

    assert!(graph.entities[0].truths.is_empty());
}
