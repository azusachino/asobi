use asobi::api::{
    GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, SearchQuery, SearchStore, TaskStore,
};
use asobi::model::{EntityInput, ObservationDeletion, ObservationInput, RelationInput};
use asobi::storage::SqliteStore;
use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;

/// Pull one entity's activity anchor into the past so a single write is
/// observable even though SQLite timestamps have one-second granularity.
fn age_last_activity(db: &std::path::Path, name: &str) {
    let conn = Connection::open(db).unwrap();
    conn.execute_batch(&format!(
        "UPDATE asobi_entities SET last_activity = datetime('now', '-2 days') WHERE name = '{name}';"
    ))
    .unwrap();
}

fn last_activity_of(db: &std::path::Path, name: &str) -> String {
    let conn = Connection::open(db).unwrap();
    conn.query_row(
        "SELECT last_activity FROM asobi_entities WHERE name = ?",
        [name],
        |r| r.get(0),
    )
    .unwrap()
}

fn store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().unwrap();
    let db = dir.path().join("contract.db");
    let store = SqliteStore::open_at(&db).unwrap();
    (dir, store)
}

#[test]
fn sqlite_implements_the_v2_contract() {
    let (_dir, store) = store();
    let capabilities = store.capabilities().unwrap();
    assert_eq!(capabilities.backend, "sqlite");
    assert_eq!(capabilities.keyword_search_kind, "fts5");
    assert!(capabilities.multi_process);
}

#[test]
fn graph_truth_search_and_task_claim_are_atomic_surfaces() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "project:asobi".into(),
            entity_type: "project".into(),
            observations: vec!["SQLite FTS5 supports concurrent agent recall".into()],
        }])
        .unwrap();
    store
        .create_entities(vec![EntityInput {
            name: "asobi:task-1".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();
    store
        .create_relations(vec![RelationInput {
            from: "asobi:task-1".into(),
            to: "project:asobi".into(),
            relation_type: "part_of".into(),
        }])
        .unwrap();
    store
        .truth_upsert("asobi:task-1", "status", "READY_TO_DISPATCH")
        .unwrap();
    store
        .truth_upsert("project:asobi", "status", "ACTIVE")
        .unwrap();

    let graph = store
        .search_nodes(SearchQuery {
            query: "concurrent recall".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    assert_eq!(graph.entities[0].name, "project:asobi");

    let filtered = store
        .search_nodes(SearchQuery {
            query: "concurrent".into(),
            limit: 10,
            filters: vec![("status".into(), "ACTIVE".into())],
        })
        .unwrap();
    assert_eq!(filtered.entities.len(), 1);
    assert_eq!(filtered.entities[0].name, "project:asobi");

    assert_eq!(
        store.claim_next("agent-a").unwrap().as_deref(),
        Some("asobi:task-1")
    );
    assert_eq!(store.claim_next("agent-b").unwrap(), None);
}

#[test]
fn graph_and_search_keep_observations_lazy() {
    let (_dir, store) = store();
    store
        .create_entities(vec![asobi::model::EntityInput {
            name: "lean-read".into(),
            entity_type: "concept".into(),
            observations: vec![],
        }])
        .unwrap();
    store
        .add_observations(
            vec![asobi::model::ObservationInput {
                entity_name: "lean-read".into(),
                contents: vec!["heavy observation".into()],
            }],
            200,
        )
        .unwrap();

    let lean = store.read_graph().unwrap();
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
        .unwrap();
    let entity = &full.entities[0];
    assert_eq!(entity.observations, vec!["heavy observation"]);
    assert_eq!(entity.observations_detailed.as_ref().unwrap().len(), 1);

    let exported = store.read_graph_full().unwrap();
    assert_eq!(exported.entities[0].observations, vec!["heavy observation"]);
}

#[test]
fn purge_is_preview_first_and_leaves_durable_knowledge() {
    let (dir, store) = store();
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
        .unwrap();
    store
        .truth_upsert("project:task-done", "status", "DONE")
        .unwrap();
    store
        .truth_upsert("project:task-closed", "status", "CLOSED")
        .unwrap();

    let db = dir.path().join("contract.db");
    let conn = Connection::open(db).unwrap();
    conn.execute_batch(
        "UPDATE asobi_entities SET created_at = datetime('now', '-90 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-90 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-90 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-90 days');",
    )
    .unwrap();
    drop(conn);

    let request = PurgeRequest {
        older_than_days: 30,
        apply: false,
    };
    let preview = store.purge(request.clone()).unwrap();
    assert!(preview.dry_run);
    assert_eq!(preview.deleted, 0);
    assert_eq!(preview.candidates.len(), 2);
    assert!(
        store
            .open_nodes(OpenNodes {
                observation_limit: 0,
                names: vec!["project:task-done".into()],
                ..Default::default()
            })
            .unwrap()
            .entities
            .len()
            == 1
    );

    let applied = store
        .purge(PurgeRequest {
            apply: true,
            ..request
        })
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
            .unwrap()
            .entities
            .is_empty()
    );
    // The purged entity leaves the index with its observations. Asserting the
    // whole result is empty would be wrong now that a multi-word query widens:
    // "note" still matches the durable concept that survived, correctly.
    assert!(
        !store
            .search_nodes(SearchQuery {
                query: "old finished task".into(),
                limit: 10,
                filters: vec![],
            })
            .unwrap()
            .entities
            .iter()
            .any(|e| e.name == "project:task")
    );
    // The durable concept survives, and there is no request that could have
    // reached it: the policy is a constant now rather than validated flags, so
    // "purge refuses durable knowledge" is structural instead of enforced.
    let survivors = store.read_graph().unwrap();
    assert_eq!(survivors.entities.len(), 1);
    assert_eq!(survivors.entities[0].name, "project:concept");
}

// storage-boundary: provider-test -- these tests read PRAGMA user_version and
// raw file bytes, which are SQLite provider detail.
#[test]
fn opening_a_pre_0_8_database_is_refused_untouched_with_a_move_aside_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");

    // Build a plausible pre-0.8 file: schema 8, with a superseded table no
    // schema-9 database would ever hold.
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE chunks (id INTEGER PRIMARY KEY);
         CREATE TABLE asobi_entities (name TEXT PRIMARY KEY);
         PRAGMA user_version = 8;",
    )
    .unwrap();
    conn.execute("INSERT INTO chunks VALUES (1)", []).unwrap();
    drop(conn);

    let before = std::fs::read(&db_path).unwrap();
    let err = match SqliteStore::open_at(&db_path) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected refusal"),
    };
    assert!(
        err.contains(&db_path.display().to_string()),
        "the error must name the path: {err}"
    );
    assert!(err.contains("before 0.8"), "{err}");
    assert!(err.contains("Move the file aside"), "{err}");
    let after = std::fs::read(&db_path).unwrap();
    assert_eq!(before, after, "a refused database must be untouched");
}

#[test]
fn opening_a_newer_database_is_refused_with_a_newer_asobi_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("from-the-future.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute_batch("PRAGMA user_version = 10;").unwrap();
    drop(conn);

    let before = std::fs::read(&db_path).unwrap();
    let err = match SqliteStore::open_at(&db_path) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected refusal"),
    };
    assert!(err.contains("newer"), "{err}");
    assert_eq!(before, std::fs::read(&db_path).unwrap());
}

#[test]
fn a_new_database_is_created_directly_at_schema_nine() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");
    {
        let store = SqliteStore::open_at(&db_path).unwrap();
        store
            .create_entities(vec![EntityInput {
                name: "project:task".into(),
                entity_type: "task".into(),
                observations: vec![],
            }])
            .unwrap();
    }
    let conn = Connection::open(&db_path).unwrap();
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_version, 9);
    let auto_vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        auto_vacuum, 2,
        "a new database should switch on incremental auto-vacuum"
    );
    let columns: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM pragma_table_info('asobi_entities')")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert!(columns.contains(&"last_activity".to_string()));
    assert!(!columns.contains(&"updated_at".to_string()));
}

#[test]
fn applied_purge_reclaims_space_via_incremental_vacuum() {
    let (dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec!["done".into()],
        }])
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .unwrap();

    let db = dir.path().join("contract.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "UPDATE asobi_entities SET created_at = datetime('now', '-90 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-90 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-90 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-90 days');",
    )
    .unwrap();
    drop(conn);

    let report = store
        .purge(PurgeRequest {
            older_than_days: 30,
            apply: true,
        })
        .unwrap();
    assert_eq!(report.deleted, 1);

    // `incremental_vacuum` after an applied purge must not error even
    // though this store was opened fresh (auto_vacuum already INCREMENTAL).
    let conn = Connection::open(&db).unwrap();
    let auto_vacuum: i64 = conn
        .query_row("PRAGMA auto_vacuum", [], |r| r.get(0))
        .unwrap();
    assert_eq!(auto_vacuum, 2);
}

/// `show` is the only eager read, and it was unbounded: loading a session at
/// the 200-observation cap cost tens of thousands of tokens to answer "where
/// was I". Context is the scarce resource, so it returns the current end of the
/// trail by default — while still reporting the true total, since a caller that
/// cannot tell what it is missing is worse off than one reading everything.
#[test]
fn show_returns_recent_observations_and_the_true_total() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:session".into(),
            entity_type: "session".into(),
            observations: vec![],
        }])
        .unwrap();
    for i in 0..50 {
        store
            .add_observations(
                vec![asobi::model::ObservationInput {
                    entity_name: "proj:session".into(),
                    contents: vec![format!("note {i}")],
                }],
                200,
            )
            .unwrap();
    }

    let limited = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 10,
            ..Default::default()
        })
        .unwrap();
    let entity = &limited.entities[0];
    assert_eq!(entity.observations.len(), 10, "should return the limit");
    assert_eq!(entity.observation_count, 50, "count is the true total");
    // Newest kept, and still in written order so a truncated trail reads forward.
    assert_eq!(entity.observations.first().unwrap(), "note 40");
    assert_eq!(entity.observations.last().unwrap(), "note 49");

    // 0 means the whole trail, which is what export relies on.
    let full = store
        .open_nodes(OpenNodes {
            names: vec!["proj:session".into()],
            observation_limit: 0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(full.entities[0].observations.len(), 50);
    assert_eq!(
        store.read_graph_full().unwrap().entities[0]
            .observations
            .len(),
        50
    );
}

/// Retention has to happen without being asked. The previous design was a
/// manual purge that was correct in every respect except that it never ran:
/// six weeks of daily use left a graph that was 96% finished work.
///
/// It fires on the first *write* rather than at open, so a pure read never
/// mutates the graph — `asobi show` must not delete anything.
#[test]
fn finished_work_is_swept_on_the_first_write_not_on_reads() {
    let (dir, store) = store();
    store
        .create_entities(vec![
            EntityInput {
                name: "project:task".into(),
                entity_type: "task".into(),
                observations: vec![],
            },
            EntityInput {
                name: "project:concept".into(),
                entity_type: "concept".into(),
                observations: vec![],
            },
        ])
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .unwrap();

    let db = dir.path().join("contract.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(
        "UPDATE asobi_entities SET created_at = datetime('now', '-30 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-30 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-30 days');",
    )
    .unwrap();
    drop(conn);

    // A fresh handle, as a new process would have.
    let reopened = SqliteStore::open_at(&db).unwrap();
    assert_eq!(
        reopened.read_graph().unwrap().entities.len(),
        2,
        "a read must not sweep"
    );

    reopened
        .truth_upsert("project:concept", "note", "anything")
        .unwrap();
    let after = reopened.read_graph().unwrap();
    assert_eq!(after.entities.len(), 1, "the finished task is gone");
    assert_eq!(after.entities[0].name, "project:concept");
}

/// Truth values are searchable. The convention is to store a pitfall's
/// human-readable warning in a `title` truth so session start can surface it
/// cheaply — which made the one sentence explaining a dead end the one thing
/// recall could not reach.
#[test]
fn search_reaches_truth_values_not_just_observations() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:pitfall:cache".into(),
            entity_type: "concept".into(),
            observations: vec!["tried: redeploying the server image".into()],
        }])
        .unwrap();
    store
        .truth_upsert(
            "proj:pitfall:cache",
            "title",
            "bump the Valkey generation manually",
        )
        .unwrap();

    let hits = |q: &str| {
        store
            .search_nodes(SearchQuery {
                query: q.into(),
                limit: 10,
                filters: vec![],
            })
            .unwrap()
            .entities
            .len()
    };
    assert_eq!(
        hits("Valkey"),
        1,
        "a token only in a truth must be findable"
    );
    assert_eq!(hits("redeploying"), 1, "observations still match");
}

/// A multi-word question must not fail closed. FTS5 ANDs bare terms, so a
/// natural-language query whose words are spread across an observation, a truth
/// and a name matched nothing — and an empty result is indistinguishable from
/// "nothing was ever recorded", which is the wrong way for a pitfall lookup to
/// fail.
#[test]
fn search_widens_rather_than_returning_a_silent_zero() {
    let (_dir, store) = store();
    store
        .create_entities(vec![EntityInput {
            name: "proj:pitfall:cache".into(),
            entity_type: "concept".into(),
            observations: vec!["tried: deploying a new image".into()],
        }])
        .unwrap();
    store
        .truth_upsert("proj:pitfall:cache", "title", "bump the cache generation")
        .unwrap();

    // No entity contains all four words, so the strict AND finds nothing.
    let widened = store
        .search_nodes(SearchQuery {
            query: "deploying without cache bump".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    assert_eq!(
        widened.entities.len(),
        1,
        "should widen rather than return nothing"
    );
    assert_eq!(widened.entities[0].name, "proj:pitfall:cache");
}

/// Ranking survives to the caller. The entity fetch used `ORDER BY name`, which
/// re-sorted results alphabetically and silently discarded whatever ranking
/// search had computed — so relevance never reached the caller at all.
#[test]
fn search_returns_results_in_ranked_order_not_alphabetical() {
    let (_dir, store) = store();
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
        .unwrap();
    store
        .truth_upsert("zzz-the-match", "title", "the widget explained")
        .unwrap();

    let ranked = store
        .search_nodes(SearchQuery {
            query: "widget".into(),
            limit: 10,
            filters: vec![],
        })
        .unwrap();
    // Alphabetically `aaa-unrelated` wins; by relevance it does not, and it is
    // matched by only one path where the other is matched by two.
    assert_eq!(ranked.entities[0].name, "zzz-the-match");
}

#[test]
fn observations_move_last_activity_in_every_direction() {
    let (dir, store) = store();
    let db = dir.path().join("contract.db");
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();

    // Add moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store
        .add_observations(
            vec![ObservationInput {
                entity_name: "project:task".into(),
                contents: vec!["first note".into()],
            }],
            200,
        )
        .unwrap();
    let after_add = last_activity_of(&db, "project:task");
    assert!(
        after_add > aged,
        "an observation add must move last_activity"
    );

    // Edit moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store
        .update_observation("project:task", "first note", "edited note")
        .unwrap();
    let after_edit = last_activity_of(&db, "project:task");
    assert!(
        after_edit > aged,
        "an observation edit must move last_activity"
    );

    // Delete moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store
        .delete_observations(vec![ObservationDeletion {
            entity_name: "project:task".into(),
            observations: vec!["edited note".into()],
        }])
        .unwrap();
    let after_delete = last_activity_of(&db, "project:task");
    assert!(after_delete > aged);
}

#[test]
fn truths_move_last_activity_in_every_direction() {
    let (dir, store) = store();
    let db = dir.path().join("contract.db");
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .unwrap();

    // Upsert (insert) moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store
        .truth_upsert("project:task", "status", "IN_PROGRESS")
        .unwrap();
    assert!(
        last_activity_of(&db, "project:task") > aged,
        "a truth insert must move last_activity"
    );

    // Upsert (update) moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store
        .truth_upsert("project:task", "status", "DONE")
        .unwrap();
    assert!(
        last_activity_of(&db, "project:task") > aged,
        "a truth update must move last_activity"
    );

    // Delete moves it.
    age_last_activity(&db, "project:task");
    let aged = last_activity_of(&db, "project:task");
    store.truth_delete("project:task", "status").unwrap();
    assert!(
        last_activity_of(&db, "project:task") > aged,
        "a truth delete must move last_activity"
    );
}

#[test]
fn relations_do_not_move_last_activity() {
    let (dir, store) = store();
    let db = dir.path().join("contract.db");
    store
        .create_entities(vec![
            EntityInput {
                name: "project:task".into(),
                entity_type: "task".into(),
                observations: vec![],
            },
            EntityInput {
                name: "project:epic".into(),
                entity_type: "task".into(),
                observations: vec![],
            },
        ])
        .unwrap();
    let before = last_activity_of(&db, "project:epic");

    let relation = RelationInput {
        from: "project:task".into(),
        to: "project:epic".into(),
        relation_type: "part_of".into(),
    };
    store.create_relations(vec![relation.clone()]).unwrap();
    assert_eq!(
        last_activity_of(&db, "project:epic"),
        before,
        "relations are not activity"
    );

    store.delete_relations(vec![relation]).unwrap();
    assert_eq!(last_activity_of(&db, "project:epic"), before);
}
