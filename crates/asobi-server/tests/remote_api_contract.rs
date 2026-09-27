use asobi::storage::RemoteStore;
use asobi_core::api::{
    ApiError, GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, SearchQuery, SearchStore,
    TaskStore,
};
use asobi_core::model::{EntityInput, ObservationInput, RelationInput};
use asobi_server::{App, BoundServer};
use std::sync::Arc;
use tempfile::tempdir;

#[tokio::test]
async fn remote_store_satisfies_the_v3_api_contract() {
    let data = tempdir().unwrap();
    let server = BoundServer::bind(
        Arc::new(App::new(data.path().to_path_buf())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let store =
        RemoteStore::new(format!("http://{}", server.local_addr), "contract".into()).unwrap();

    assert_eq!(store.capabilities().await.unwrap().backend, "sqlite");
    assert!(store.health().await.unwrap().reachable);
    assert_eq!(store.location().await.unwrap().schema_version, 9);

    store
        .create_entities(vec![
            EntityInput {
                name: "project:contract".into(),
                entity_type: "project".into(),
                observations: vec!["remote contract seed".into()],
            },
            EntityInput {
                name: "project:contract:task-1".into(),
                entity_type: "task".into(),
                observations: vec![],
            },
        ])
        .await
        .unwrap();
    store
        .truth_upsert("project:contract", "status", "ACTIVE")
        .await
        .unwrap();
    store
        .truth_upsert("project:contract:task-1", "status", "READY_TO_DISPATCH")
        .await
        .unwrap();
    store
        .create_relations(vec![RelationInput {
            from: "project:contract:task-1".into(),
            to: "project:contract".into(),
            relation_type: "part_of".into(),
        }])
        .await
        .unwrap();

    store
        .add_observations(
            vec![ObservationInput {
                entity_name: "project:contract".into(),
                contents: vec!["update-me".into()],
            }],
            200,
        )
        .await
        .unwrap();
    let detail = store
        .open_nodes(OpenNodes {
            names: vec!["project:contract".into()],
            with_ids: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let observation_id = detail.entities[0]
        .observations_detailed
        .as_ref()
        .unwrap()
        .iter()
        .find(|observation| observation.content == "update-me")
        .unwrap()
        .id;
    store
        .update_observation_by_id("project:contract", observation_id, "updated-by-id")
        .await
        .unwrap();
    store
        .update_observation("project:contract", "updated-by-id", "updated-by-content")
        .await
        .unwrap();

    let graph = store.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 2);
    assert_eq!(graph.relations.len(), 1);
    assert!(graph.entities[0].observations.is_empty());
    assert_eq!(
        graph.entities[0].observation_count, 2,
        "read_graph returns the true count without loading observations"
    );
    let full = store.read_graph_full().await.unwrap();
    assert!(
        full.entities[0]
            .observations
            .contains(&"updated-by-content".into())
    );
    let search = store
        .search_nodes(SearchQuery {
            query: "remote contract".into(),
            limit: 10,
            filters: vec![("status".into(), "ACTIVE".into())],
        })
        .await
        .unwrap();
    assert_eq!(search.entities.len(), 1);
    assert_eq!(search.entities[0].name, "project:contract");

    store
        .delete_observation_by_id("project:contract", observation_id)
        .await
        .unwrap();
    store
        .delete_observations(vec![asobi_core::model::ObservationDeletion {
            entity_name: "project:contract".into(),
            observations: vec!["remote contract seed".into()],
        }])
        .await
        .unwrap();
    store
        .truth_delete("project:contract", "status")
        .await
        .unwrap();
    store
        .delete_relations(vec![RelationInput {
            from: "project:contract:task-1".into(),
            to: "project:contract".into(),
            relation_type: "part_of".into(),
        }])
        .await
        .unwrap();

    assert_eq!(
        store
            .dispatch(None, "contract-agent", 200)
            .await
            .unwrap()
            .as_deref(),
        Some("project:contract:task-1")
    );
    assert_eq!(store.claim_next("contract-agent").await.unwrap(), None);
    let preview = store
        .purge(PurgeRequest {
            older_than_days: 30,
            apply: false,
        })
        .await
        .unwrap();
    assert!(preview.dry_run);
    assert!(preview.candidates.is_empty());
    let stats = store.stats().await.unwrap();
    assert_eq!(stats.entities, 2);
    assert!(store.stats_per_entity().await.unwrap().len() >= 2);
    assert!(matches!(store.reset().await, Err(ApiError::Unsupported(_))));

    store
        .delete_entities(vec![
            "project:contract:task-1".into(),
            "project:contract".into(),
        ])
        .await
        .unwrap();
    server.shutdown().await;
}
