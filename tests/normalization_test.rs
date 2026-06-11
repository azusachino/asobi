use miku::db::{ENV_DATABASE_URL, init_db, mcp_create_entities, mcp_read_graph};
use miku::mcp::EntityInput;
use tempfile::tempdir;

#[tokio::test]
async fn test_entity_name_normalization() {
    let dir = tempdir().unwrap();
    unsafe {
        std::env::set_var(
            ENV_DATABASE_URL,
            dir.path().join("test.db").to_str().unwrap(),
        );
    }
    let (_db, conn) = init_db().await.unwrap();

    let entities = vec![EntityInput {
        name: "User Preferences".to_string(),
        entity_type: "concept".to_string(),
        observations: vec!["test obs".to_string()],
    }];

    mcp_create_entities(&conn, entities).await.unwrap();

    let graph = mcp_read_graph(&conn).await.unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "User-Preferences");
}
