use asobi::db::{ENV_DATABASE_URL, create_entities, init_db, read_graph};
use asobi::model::EntityInput;
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

    create_entities(&conn, entities).await.unwrap();

    let graph = read_graph(&conn).await.unwrap();
    assert_eq!(graph.entities.len(), 1);
    assert_eq!(graph.entities[0].name, "User-Preferences");
}
