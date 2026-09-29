use asobi_core::api::{
    GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, SearchQuery, SearchStore, TaskStore,
};
use asobi_core::model::{EntityInput, ObservationInput, RelationInput};

pub async fn stats_capabilities_health_and_location_contract<S: GraphStore + MaintenanceStore>(
    store: &S,
) {
    let capabilities = store.capabilities().await.unwrap();
    assert_eq!(capabilities.backend, "sqlite");
    assert_eq!(capabilities.keyword_search_kind, "fts5");
    assert!(capabilities.multi_process);

    let health = store.health().await.unwrap();
    assert_eq!(health.backend, "sqlite");
    assert!(health.reachable);
    let location = store.location().await.unwrap();
    assert_eq!(location.schema_version, 9);
    assert_eq!(location.journal_mode.to_ascii_lowercase(), "wal");

    let empty = store.stats().await.unwrap();
    assert_eq!(empty.entities, 0);
    assert_eq!(empty.relations, 0);
    assert_eq!(empty.observations, 0);
    assert!(store.stats_per_entity().await.unwrap().is_empty());

    store
        .create_entities(vec![EntityInput {
            name: "contract:stats".into(),
            entity_type: "concept".into(),
            observations: vec!["one observation".into()],
        }])
        .await
        .unwrap();
    let stats = store.stats().await.unwrap();
    assert_eq!(stats.entities, 1);
    assert_eq!(stats.observations, 1);
    assert_eq!(store.stats_per_entity().await.unwrap().len(), 1);
}

pub async fn graph_truth_search_and_task_claim_are_atomic_surfaces<S>(store: &S)
where
    S: GraphStore + SearchStore + TaskStore,
{
    store
        .create_entities(vec![EntityInput {
            name: "project:asobi".into(),
            entity_type: "project".into(),
            observations: vec!["SQLite FTS5 supports concurrent agent recall".into()],
        }])
        .await
        .unwrap();
    store
        .create_entities(vec![EntityInput {
            name: "asobi:task-1".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    store
        .create_relations(vec![RelationInput {
            from: "asobi:task-1".into(),
            to: "project:asobi".into(),
            relation_type: "part_of".into(),
        }])
        .await
        .unwrap();
    store
        .truth_upsert("asobi:task-1", "status", "READY_TO_DISPATCH")
        .await
        .unwrap();
    store
        .truth_upsert("project:asobi", "status", "ACTIVE")
        .await
        .unwrap();

    let graph = store
        .search_nodes(SearchQuery {
            query: "concurrent recall".into(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    assert_eq!(graph.entities[0].name, "project:asobi");

    let filtered = store
        .search_nodes(SearchQuery {
            query: "concurrent".into(),
            limit: 10,
            filters: vec![("status".into(), "ACTIVE".into())],
        })
        .await
        .unwrap();
    assert_eq!(filtered.entities.len(), 1);
    assert_eq!(filtered.entities[0].name, "project:asobi");

    assert_eq!(
        store.claim_next("agent-a").await.unwrap().as_deref(),
        Some("asobi:task-1")
    );
    assert_eq!(store.claim_next("agent-b").await.unwrap(), None);
}

pub async fn task_update_is_one_operation_and_preserves_status_without_explicit_change<S>(store: &S)
where
    S: GraphStore + TaskStore,
{
    store
        .create_entities(vec![EntityInput {
            name: "contract:handoff".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    store
        .truth_upsert("contract:handoff", "status", "DISPATCHED")
        .await
        .unwrap();
    let status = store
        .update("contract:handoff", vec!["tests passed".into()], None, 200)
        .await
        .unwrap();
    assert_eq!(status, "DISPATCHED");
    let task = store
        .open_nodes(OpenNodes {
            names: vec!["contract:handoff".into()],
            observation_limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(task.entities[0].truths["status"], "DISPATCHED");
    assert_eq!(task.entities[0].observations, ["tests passed"]);
    assert!(
        store
            .update(
                "contract:handoff",
                vec!["ignored".into()],
                Some("NOT_A_STATUS"),
                200
            )
            .await
            .is_err()
    );
    let task = store
        .open_nodes(OpenNodes {
            names: vec!["contract:handoff".into()],
            observation_limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(task.entities[0].observations, ["tests passed"]);
    assert_eq!(
        store
            .update(
                "contract:handoff",
                vec!["reviewed".into()],
                Some("DONE"),
                200
            )
            .await
            .unwrap(),
        "DONE"
    );
    let task = store
        .open_nodes(OpenNodes {
            names: vec!["contract:handoff".into()],
            observation_limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(task.entities[0].truths["status"], "DONE");
    assert_eq!(task.entities[0].observations, ["tests passed", "reviewed"]);
}

pub async fn graph_and_search_keep_observations_lazy<S>(store: &S)
where
    S: GraphStore + SearchStore,
{
    store
        .create_entities(vec![EntityInput {
            name: "lean-read".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    store
        .add_observations(
            vec![ObservationInput {
                entity_name: "lean-read".into(),
                contents: vec!["heavy observation".into()],
            }],
            200,
        )
        .await
        .unwrap();

    let lean = store.read_graph().await.unwrap();
    let entity = &lean.entities[0];
    assert_eq!(entity.observation_count, 1);
    assert!(entity.observations.is_empty());
    assert!(entity.observations_detailed.is_none());
    let lean_json = serde_json::to_value(&lean).unwrap();
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("body")
    );
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("observations")
    );
    assert!(
        !lean_json["entities"][0]
            .as_object()
            .unwrap()
            .contains_key("observationsDetailed")
    );

    let search = store
        .search_nodes(SearchQuery {
            query: "heavy observation".into(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    let entity = &search.entities[0];
    assert_eq!(entity.observation_count, 1);
    assert!(entity.observations.is_empty());
    assert!(entity.observations_detailed.is_none());

    let full = store
        .open_nodes(OpenNodes {
            observation_limit: 0,
            names: vec!["lean-read".into()],
            with_ids: true,
            expand: vec![],
        })
        .await
        .unwrap();
    let entity = &full.entities[0];
    assert_eq!(entity.observations, vec!["heavy observation"]);
    assert_eq!(entity.observations_detailed.as_ref().unwrap().len(), 1);

    let exported = store.read_graph_full().await.unwrap();
    assert_eq!(exported.entities[0].observations, vec!["heavy observation"]);
}

pub async fn purge_is_preview_first_and_leaves_durable_knowledge<S>(store: &S)
where
    S: GraphStore + SearchStore + MaintenanceStore,
{
    store
        .create_entities(vec![
            EntityInput {
                name: "project:task-done".into(),
                entity_type: "task".into(),
                observations: vec!["old finished task".into()],
            },
            EntityInput {
                name: "project:task-closed".into(),
                entity_type: "task".into(),
                observations: vec!["old closed task".into()],
            },
            EntityInput {
                name: "project:concept".into(),
                entity_type: "concept".into(),
                observations: vec!["durable note".into()],
            },
        ])
        .await
        .unwrap();
    store
        .truth_upsert("project:task-done", "status", "DONE")
        .await
        .unwrap();
    store
        .truth_upsert("project:task-closed", "status", "CLOSED")
        .await
        .unwrap();

    // Purge uses second-resolution activity timestamps. Waiting lets the
    // public API create an eligible zero-day candidate without SQLite-only SQL.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let request = PurgeRequest {
        older_than_days: 0,
        apply: false,
    };
    let preview = store.purge(request.clone()).await.unwrap();
    assert!(preview.dry_run);
    assert_eq!(preview.deleted, 0);
    assert_eq!(preview.candidates.len(), 2);
    assert_eq!(
        store
            .open_nodes(OpenNodes {
                observation_limit: 0,
                names: vec!["project:task-done".into()],
                ..Default::default()
            })
            .await
            .unwrap()
            .entities
            .len(),
        1
    );

    let applied = store
        .purge(PurgeRequest {
            apply: true,
            ..request
        })
        .await
        .unwrap();
    assert!(!applied.dry_run);
    assert_eq!(applied.deleted, 2);
    assert!(
        store
            .open_nodes(OpenNodes {
                observation_limit: 0,
                names: vec!["project:task-done".into()],
                ..Default::default()
            })
            .await
            .unwrap()
            .entities
            .is_empty()
    );
    assert!(
        !store
            .search_nodes(SearchQuery {
                query: "old finished task".into(),
                limit: 10,
                filters: vec![],
            })
            .await
            .unwrap()
            .entities
            .iter()
            .any(|entity| entity.name == "project:task-done")
    );

    let survivors = store.read_graph().await.unwrap();
    assert_eq!(survivors.entities.len(), 1);
    assert_eq!(survivors.entities[0].name, "project:concept");
}

pub async fn show_returns_recent_observations_and_the_true_total<S: GraphStore>(store: &S) {
    store
        .create_entities(vec![EntityInput {
            name: "proj:session".into(),
            entity_type: "session".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    for i in 0..50 {
        store
            .add_observations(
                vec![ObservationInput {
                    entity_name: "proj:session".into(),
                    contents: vec![format!("note {i}")],
                }],
                200,
            )
            .await
            .unwrap();
    }

    let limited = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 10,
            ..Default::default()
        })
        .await
        .unwrap();
    let entity = &limited.entities[0];
    assert_eq!(entity.observations.len(), 10, "should return the limit");
    assert_eq!(entity.observation_count, 50, "count is the true total");
    assert_eq!(entity.observations.first().unwrap(), "note 40");
    assert_eq!(entity.observations.last().unwrap(), "note 49");

    let full = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 0,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(full.entities[0].observations.len(), 50);
    assert_eq!(
        store.read_graph_full().await.unwrap().entities[0]
            .observations
            .len(),
        50
    );
}

pub async fn search_reaches_truth_values_not_just_observations<S>(store: &S)
where
    S: GraphStore + SearchStore,
{
    store
        .create_entities(vec![EntityInput {
            name: "proj:pitfall:cache".into(),
            entity_type: "concept".into(),
            observations: vec!["tried: redeploying the server image".into()],
        }])
        .await
        .unwrap();
    store
        .truth_upsert(
            "proj:pitfall:cache",
            "title",
            "bump the Valkey generation manually",
        )
        .await
        .unwrap();

    assert_eq!(
        hits(store, "Valkey").await,
        1,
        "a token only in a truth must be findable"
    );
    assert_eq!(
        hits(store, "redeploying").await,
        1,
        "observations still match"
    );
}

async fn hits<S: SearchStore>(store: &S, query: &str) -> usize {
    store
        .search_nodes(SearchQuery {
            query: query.into(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap()
        .entities
        .len()
}

pub async fn search_widens_rather_than_returning_a_silent_zero<S>(store: &S)
where
    S: GraphStore + SearchStore,
{
    store
        .create_entities(vec![EntityInput {
            name: "proj:pitfall:cache".into(),
            entity_type: "concept".into(),
            observations: vec!["tried: deploying a new image".into()],
        }])
        .await
        .unwrap();
    store
        .truth_upsert("proj:pitfall:cache", "title", "bump the cache generation")
        .await
        .unwrap();

    let widened = store
        .search_nodes(SearchQuery {
            query: "deploying without cache bump".into(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        widened.entities.len(),
        1,
        "should widen rather than return nothing"
    );
    assert_eq!(widened.entities[0].name, "proj:pitfall:cache");
}

pub async fn search_returns_results_in_ranked_order_not_alphabetical<S>(store: &S)
where
    S: GraphStore + SearchStore,
{
    store
        .create_entities(vec![
            EntityInput {
                name: "aaa-unrelated".into(),
                entity_type: "concept".into(),
                observations: vec!["mentions widget once".into()],
            },
            EntityInput {
                name: "zzz-the-match".into(),
                entity_type: "concept".into(),
                observations: vec!["widget widget widget, entirely about the widget".into()],
            },
        ])
        .await
        .unwrap();
    store
        .truth_upsert("zzz-the-match", "title", "the widget explained")
        .await
        .unwrap();

    let ranked = store
        .search_nodes(SearchQuery {
            query: "widget".into(),
            limit: 10,
            filters: vec![],
        })
        .await
        .unwrap();
    assert_eq!(ranked.entities[0].name, "zzz-the-match");
}
