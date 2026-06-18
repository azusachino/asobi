use asobi::db::{
    ENV_DATABASE_URL, init_db, mcp_create_entities, mcp_create_relations, mcp_search_nodes,
};
use asobi::model::{EntityInput, RelationInput};
use tempfile::tempdir;

#[tokio::test]
async fn test_search_nodes_expands_neighbors() {
    let dir = tempdir().unwrap();
    unsafe {
        std::env::set_var(
            ENV_DATABASE_URL,
            dir.path().join("test.db").to_str().unwrap(),
        );
    }
    let (_db, conn) = init_db().await.unwrap();

    // Create 2 entities and a relation
    mcp_create_entities(
        &conn,
        vec![
            EntityInput {
                name: "source".to_string(),
                entity_type: "node".to_string(),
                observations: vec!["source node".to_string()],
            },
            EntityInput {
                name: "target".to_string(),
                entity_type: "node".to_string(),
                observations: vec!["target node".to_string()],
            },
        ],
    )
    .await
    .unwrap();

    mcp_create_relations(
        &conn,
        vec![RelationInput {
            from: "source".to_string(),
            to: "target".to_string(),
            relation_type: "connects_to".to_string(),
        }],
    )
    .await
    .unwrap();

    // Search for "source" — should return source and target via neighbor expansion
    let graph = mcp_search_nodes(&conn, "source").await.unwrap();

    assert_eq!(
        graph.entities.len(),
        2,
        "Should have retrieved both source and target"
    );
    assert_eq!(
        graph.relations.len(),
        1,
        "Should have retrieved the connecting relation"
    );
    assert!(graph.relations[0].from == "source" && graph.relations[0].to == "target");
}
