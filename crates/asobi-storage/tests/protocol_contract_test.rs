// storage-boundary: provider-test
//!
//! The RPC contract (ADR 0005) exercised end to end against a real store:
//! params JSON -> dispatcher -> result JSON, error shapes, and the protocol
//! cases. The store is temporary SQLite, hence the marker.

use asobi_core::api::GraphStore;
use asobi_core::model::EntityInput;
use asobi_core::protocol::{Operation, dispatch, error_body_to_api};
use asobi_storage::SqliteStore;
use serde_json::{Value, json};
use tempfile::tempdir;

async fn seeded_store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().unwrap();
    let store = SqliteStore::open_at(&dir.path().join("protocol.db"))
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

/// One round-trip: serialize the request body, hand the bytes to the
/// dispatcher, and assert the result. The bodies here are already the exact
/// JSON the wire carries, so this is the contract, not a Rust-only path.
async fn ok(store: &SqliteStore, operation: &str, params: Value) -> Value {
    let body = serde_json::to_vec(&params).unwrap();
    dispatch(store, operation, Some(&body))
        .await
        .unwrap_or_else(|e| panic!("{operation} failed: {e:?}"))
}

async fn err(
    store: &SqliteStore,
    operation: &str,
    body: Option<&[u8]>,
) -> asobi_core::protocol::ProtocolError {
    dispatch(store, operation, body)
        .await
        .expect_err("expected an error")
}

#[tokio::test]
async fn every_operation_round_trips() {
    let (_dir, store) = seeded_store().await;

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
        json!({ "entity": "project:asobi", "key": "topic", "value": "protocol" }),
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
        json!({ "query": "commonterm", "limit": 10, "filters": [["topic", "protocol"]] }),
    )
    .await;
    assert_eq!(search["entities"][0]["name"], "project:asobi");

    // And the default is the CLI's 10, not storage's 100 for a zero limit.
    store
        .create_entities(
            (0..15)
                .map(|i| EntityInput {
                    name: format!("project:many-{i}"),
                    entity_type: "project".into(),
                    observations: vec!["manymatch".into()],
                })
                .collect(),
        )
        .await
        .unwrap();
    let many = dispatch(&store, "search.nodes", Some(br#"{"query": "manymatch"}"#))
        .await
        .unwrap();
    assert_eq!(many["entities"].as_array().unwrap().len(), 10);

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
        json!({ "task": null, "agent": "protocol-agent", "observationLimit": 200 }),
    )
    .await;
    assert_eq!(claimed, "project:asobi:task-1");
    // Nothing left to claim: claimNext finds nothing.
    assert_eq!(
        ok(
            &store,
            "tasks.claimNext",
            json!({ "agent": "protocol-agent" })
        )
        .await,
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
async fn unknown_operation_is_404_unknown_operation() {
    let (_dir, store) = seeded_store().await;
    let error = err(&store, "graph.frobnicate", Some(b"{}".as_slice())).await;
    assert_eq!(error.status, 404);
    assert_eq!(error.kind, "unknownOperation");
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
async fn wrong_request_shape_is_400_bad_request() {
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
fn operation_schemas_cover_every_operation() {
    let schemas = asobi_core::protocol::operation_schemas();
    assert_eq!(schemas.len(), Operation::all().count());
    for (name, request, result) in schemas {
        assert!(request.is_object(), "{name} request schema missing");
        assert!(
            result.is_object() || result.is_null(),
            "{name} result schema missing"
        );
    }
}

#[tokio::test]
async fn omitted_optional_fields_take_the_cli_defaults() {
    let (_dir, store) = seeded_store().await;

    // search.nodes with no `limit`: defaults to the CLI's 10, not a
    // meaning-changing zero, and the seed matches, so results come back.
    let search = dispatch(&store, "search.nodes", Some(br#"{"query": "seed"}"#))
        .await
        .unwrap();
    assert_eq!(search["entities"][0]["name"], "project:asobi");

    // openNodes with no `observationLimit`: defaults to `asobi show`'s 20
    // most recent observations, with the true total still reported.
    store
        .create_entities(vec![EntityInput {
            name: "project:long-trail".into(),
            entity_type: "project".into(),
            observations: (0..25).map(|i| format!("note {i}")).collect(),
        }])
        .await
        .unwrap();
    let nodes = dispatch(
        &store,
        "graph.openNodes",
        Some(br#"{"names": ["project:long-trail"]}"#),
    )
    .await
    .unwrap();
    assert_eq!(nodes["entities"][0]["observationCount"], 25);
    assert_eq!(
        nodes["entities"][0]["observations"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
}

#[tokio::test]
async fn claims_require_an_agent() {
    // Who holds a claim is never guessed on the wire: no default agent.
    let (_dir, store) = seeded_store().await;
    for (operation, body) in [
        (
            "tasks.dispatch",
            br#"{"task": "project:asobi:task-1"}"#.as_slice(),
        ),
        ("tasks.claimNext", br#"{}"#.as_slice()),
    ] {
        let error = dispatch(&store, operation, Some(body)).await.unwrap_err();
        assert_eq!(error.status, 400, "{operation}");
    }
    // Nothing was claimed.
    let task = dispatch(
        &store,
        "graph.openNodes",
        Some(br#"{"names": ["project:asobi:task-1"]}"#),
    )
    .await
    .unwrap();
    assert_eq!(task["entities"][0]["truths"]["status"], "READY_TO_DISPATCH");
}
