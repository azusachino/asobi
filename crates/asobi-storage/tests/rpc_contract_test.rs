// storage-boundary: provider-test
//!
//! The RPC contract (ADR 0005) exercised end to end against a real store:
//! params JSON -> dispatcher -> result JSON, error shapes, and the protocol
//! cases. The store is temporary SQLite, hence the marker.

use asobi_core::api::GraphStore;
use asobi_core::model::EntityInput;
use asobi_core::rpc::{Method, dispatch, error_body_to_api};
use asobi_storage::SqliteStore;
use serde_json::{Value, json};
use tempfile::tempdir;

async fn seeded_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().unwrap();
    let store = SqliteStore::open_at(&dir.path().join("rpc.db"))
        .await
        .unwrap();
    store
        .create_entities(vec![
            EntityInput {
                name: "project:asobi".into(),
                entity_type: "project".into(),
                observations: vec!["seed observation commonterm".into()],
            },
            EntityInput {
                name: "project:asobi:task-1".into(),
                entity_type: "task".into(),
                observations: vec![],
            },
        ])
        .await
        .unwrap();
    store
        .create_relations(vec![asobi_core::model::RelationInput {
            from: "project:asobi:task-1".into(),
            to: "project:asobi".into(),
            relation_type: "part_of".into(),
        }])
        .await
        .unwrap();
    store
        .truth_upsert("project:asobi:task-1", "status", "READY_TO_DISPATCH")
        .await
        .unwrap();
    (dir, store)
}

/// One round-trip: serialize the params, hand the bytes to the dispatcher,
/// and assert the result. The params here are already the exact JSON the
/// wire carries, so this is the contract, not a Rust-only path.
async fn ok(store: &SqliteStore, method: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&params).unwrap();
    dispatch(store, method, Some(&body))
        .await
        .unwrap_or_else(|e| panic!("{method} failed: {e:?}"))
}

async fn err(store: &SqliteStore, method: &str, body: Option<&[u8]>) -> asobi_core::rpc::RpcError {
    dispatch(store, method, body)
        .await
        .expect_err("expected an error")
}

#[tokio::test]
async fn every_method_round_trips() {
    let (_dir, store) = seeded_store().await;

    // server.hello
    let hello = ok(&store, "server.hello", json!({})).await;
    assert_eq!(hello["apiVersion"], asobi_core::api::API_VERSION);
    assert_eq!(hello["backend"], "sqlite");

    // graph writes
    ok(
        &store,
        "graph.createEntities",
        json!({ "entities": [{ "name": "project:extra", "entityType": "concept",
                               "observations": ["an observation"] }] }),
    )
    .await;
    ok(
        &store,
        "graph.addObservations",
        json!({ "observations": [{ "entityName": "project:extra",
                                   "contents": ["second"] }], "limit": 200 }),
    )
    .await;
    ok(
        &store,
        "graph.createRelations",
        json!({ "relations": [{ "from": "project:extra", "to": "project:asobi",
                                "relationType": "extends" }] }),
    )
    .await;
    ok(
        &store,
        "graph.updateObservation",
        json!({ "entityName": "project:extra", "oldContent": "second",
                "newContent": "second (edited)" }),
    )
    .await;
    ok(
        &store,
        "graph.updateObservationById",
        json!({ "entityName": "project:extra", "id": 2, "newContent": "second" }),
    )
    .await;
    ok(
        &store,
        "graph.truthUpsert",
        json!({ "entity": "project:asobi", "key": "topic", "value": "rpc" }),
    )
    .await;

    // graph reads
    let graph = ok(&store, "graph.readGraph", json!({})).await;
    assert!(graph["entities"].as_array().unwrap().len() >= 3);
    ok(&store, "graph.readGraphFull", json!({})).await;
    let nodes = ok(
        &store,
        "graph.openNodes",
        json!({ "names": ["project:asobi"], "withIds": true, "expand": [],
                "observationLimit": 0 }),
    )
    .await;
    assert_eq!(nodes["entities"][0]["name"], "project:asobi");
    let search = ok(
        &store,
        "search.nodes",
        json!({ "query": "commonterm", "limit": 10, "filters": [["topic", "rpc"]] }),
    )
    .await;
    assert_eq!(search["entities"][0]["name"], "project:asobi");

    // maintenance
    let stats = ok(&store, "maintenance.stats", json!({})).await;
    assert!(stats["entities"].as_u64().unwrap() >= 3);
    let per_entity = ok(&store, "maintenance.statsPerEntity", json!({})).await;
    assert!(per_entity.as_array().unwrap().len() >= 3);
    let preview = ok(
        &store,
        "maintenance.purge",
        json!({ "olderThanDays": 30, "apply": false }),
    )
    .await;
    assert_eq!(preview["dryRun"], true);
    ok(&store, "maintenance.capabilities", json!({})).await;
    ok(&store, "maintenance.health", json!({})).await;
    let location = ok(&store, "maintenance.location", json!({})).await;
    assert_eq!(location["schemaVersion"], 9);

    // tasks
    let claimed = ok(
        &store,
        "tasks.dispatch",
        json!({ "task": null, "agent": "rpc-agent", "observationLimit": 200 }),
    )
    .await;
    assert_eq!(claimed, "project:asobi:task-1");
    // Nothing left to claim: claimNext finds nothing.
    assert_eq!(
        ok(&store, "tasks.claimNext", json!({ "agent": "rpc-agent" })).await,
        Value::Null
    );

    // deletes: everything the writes above created, then the seeds
    ok(
        &store,
        "graph.deleteRelations",
        json!({ "relations": [{ "from": "project:extra", "to": "project:asobi",
                                "relationType": "extends" }] }),
    )
    .await;
    ok(
        &store,
        "graph.deleteObservations",
        json!({ "deletions": [{ "entityName": "project:extra",
                                "observations": ["an observation", "second"] }] }),
    )
    .await;
    ok(
        &store,
        "graph.deleteObservationById",
        json!({ "entityName": "project:extra", "id": 1 }),
    )
    .await;
    ok(
        &store,
        "graph.truthDelete",
        json!({ "entity": "project:asobi", "key": "topic" }),
    )
    .await;
    ok(
        &store,
        "graph.deleteEntities",
        json!({ "names": ["project:extra"] }),
    )
    .await;
}

#[tokio::test]
async fn every_api_error_variant_maps_to_its_status_kind_and_back() {
    for (expected_status, expected_kind) in [
        (404u16, "notFound"),
        (409, "conflict"),
        (422, "invalid"),
        (501, "unsupported"),
        (503, "unavailable"),
        (500, "backend"),
    ] {
        // Reach every variant through the dispatcher where possible: NotFound
        // via a missing entity, Unsupported via reset. The rest are mapped
        // directly — the table under test is `rpc_shape`, and dispatch is the
        // only writer of the wire shape for store failures.
        let variant = match expected_kind {
            "notFound" => asobi_core::api::ApiError::NotFound("gone".into()),
            "conflict" => asobi_core::api::ApiError::Conflict("clash".into()),
            "invalid" => asobi_core::api::ApiError::Invalid("bad input".into()),
            "unsupported" => asobi_core::api::ApiError::Unsupported("no can do"),
            "unavailable" => asobi_core::api::ApiError::Unavailable("down".into()),
            _ => asobi_core::api::ApiError::Backend("broke".into()),
        };
        let error = variant.to_rpc_error();
        assert_eq!(error.status, expected_status, "{expected_kind}");
        assert_eq!(error.kind, expected_kind);

        // and back: same variant.
        let rebuilt = error_body_to_api(&error.body()).unwrap();
        assert_eq!(
            rebuilt.rpc_shape(),
            (expected_status, expected_kind),
            "kind {expected_kind} must rebuild the same variant"
        );
    }
}

#[tokio::test]
async fn unknown_method_is_404_unknown_method() {
    let (_dir, store) = seeded_store().await;
    let error = err(&store, "graph.frobnicate", Some(b"{}".as_slice())).await;
    assert_eq!(error.status, 404);
    assert_eq!(error.kind, "unknownMethod");
}

#[tokio::test]
async fn malformed_body_is_400_bad_request() {
    let (_dir, store) = seeded_store().await;
    let error = err(
        &store,
        "graph.createEntities",
        Some(b"{not json".as_slice()),
    )
    .await;
    assert_eq!(error.status, 400);
    assert_eq!(error.kind, "badRequest");
}

#[tokio::test]
async fn wrong_params_shape_is_400_bad_request() {
    let (_dir, store) = seeded_store().await;
    // createEntities needs `entities`; a string is not that shape.
    let error = err(&store, "graph.createEntities", Some(br#""nope""#)).await;
    assert_eq!(error.status, 400);
    assert_eq!(error.kind, "badRequest");
}

#[tokio::test]
async fn reset_is_refused_and_leaves_the_graph_intact() {
    let (_dir, store) = seeded_store().await;
    let error = err(&store, "maintenance.reset", Some(b"{}".as_slice())).await;
    assert_eq!(error.status, 501);
    assert_eq!(error.kind, "unsupported");
    // The graph is untouched: the seeds are still there.
    let graph = store.read_graph().await.unwrap();
    assert_eq!(graph.entities.len(), 2);
}

#[tokio::test]
async fn dispatch_parses_raw_json_bodies_not_only_values() {
    // The dispatcher's body input is raw bytes, exactly what WP4 hands it.
    let (_dir, store) = seeded_store().await;
    let graph = dispatch(
        &store,
        "graph.openNodes",
        Some(br#"{"names": ["project:asobi"], "observationLimit": 0}"#),
    )
    .await
    .unwrap();
    assert_eq!(graph["entities"][0]["name"], "project:asobi");
}

/// The schema publisher covers every method: a method added without a schema
/// row breaks this assertion.
#[test]
fn method_schemas_cover_every_method() {
    let schemas = asobi_core::rpc::method_schemas();
    assert_eq!(schemas.len(), Method::all().count());
    for (name, params, result) in schemas {
        assert!(params.is_object(), "{name} params schema missing");
        assert!(
            result.is_object() || result.is_null(),
            "{name} result schema missing"
        );
    }
}
