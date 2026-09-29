use asobi_core::api::{GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, TaskStore};
use asobi_core::model::{EntityInput, ObservationDeletion, ObservationInput, RelationInput};
use asobi_storage::SqliteStore;
use sqlx::Connection;
use sqlx::sqlite::SqliteConnection;
use tempfile::tempdir;

#[path = "../../asobi-core/tests/support/backend_api_contract.rs"]
mod shared_contract;

/// Pull one entity's activity anchor into the past so a single write is
/// observable even though SQLite timestamps have one-second granularity.
async fn age_last_activity(db: &std::path::Path, name: &str) {
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE asobi_entities SET last_activity = datetime('now', '-2 days') WHERE name = '{name}';"
    )))
    .execute(&mut conn)
    .await
    .unwrap();
}

async fn last_activity_of(db: &std::path::Path, name: &str) -> String {
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    sqlx::query_scalar("SELECT last_activity FROM asobi_entities WHERE name = ?")
        .bind(name)
        .fetch_one(&mut conn)
        .await
        .unwrap()
}

async fn store() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempdir().unwrap();
    let db = dir.path().join("contract.db");
    let store = SqliteStore::open_at(&db).await.unwrap();
    (dir, store)
}

#[tokio::test]
async fn sqlite_implements_the_v3_contract() {
    let (_dir, store) = store().await;
    shared_contract::stats_capabilities_health_and_location_contract(&store).await;
}

#[tokio::test]
async fn graph_truth_search_and_task_claim_are_atomic_surfaces() {
    let (_dir, store) = store().await;
    shared_contract::graph_truth_search_and_task_claim_are_atomic_surfaces(&store).await;
}

#[tokio::test]
async fn task_update_is_one_operation_and_preserves_status_without_explicit_change() {
    let (_dir, store) = store().await;
    shared_contract::task_update_is_one_operation_and_preserves_status_without_explicit_change(
        &store,
    )
    .await;
}

#[tokio::test]
async fn failed_status_write_rolls_back_task_notes() {
    let (dir, store) = store().await;
    store
        .create_entities(vec![EntityInput {
            name: "rollback:task".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    store
        .truth_upsert("rollback:task", "status", "DISPATCHED")
        .await
        .unwrap();
    let db = dir.path().join("contract.db");
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER reject_task_status BEFORE UPDATE ON asobi_truths \
         WHEN NEW.key='status' BEGIN SELECT RAISE(ABORT, 'blocked'); END",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    let result = store
        .update(
            "rollback:task",
            vec!["must roll back".into()],
            Some("DONE"),
            200,
        )
        .await;
    assert!(result.is_err());
    let task = store
        .open_nodes(OpenNodes {
            names: vec!["rollback:task".into()],
            observation_limit: 20,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(task.entities[0].truths["status"], "DISPATCHED");
    assert!(task.entities[0].observations.is_empty());
}

#[tokio::test]
async fn graph_and_search_keep_observations_lazy() {
    let (_dir, store) = store().await;
    shared_contract::graph_and_search_keep_observations_lazy(&store).await;
}

#[tokio::test]
async fn purge_is_preview_first_and_leaves_durable_knowledge() {
    let (_dir, store) = store().await;
    shared_contract::purge_is_preview_first_and_leaves_durable_knowledge(&store).await;
}

// storage-boundary: provider-test -- these tests read PRAGMA user_version and
// raw file bytes, which are SQLite provider detail.
#[tokio::test]
async fn opening_a_pre_0_8_database_is_refused_untouched_with_a_move_aside_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("legacy.db");

    // Build a plausible pre-0.8 file: schema 8, with a superseded table no
    // schema-9 database would ever hold.
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE chunks (id INTEGER PRIMARY KEY);
         CREATE TABLE asobi_entities (name TEXT PRIMARY KEY);
         PRAGMA user_version = 8;",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    sqlx::query("INSERT INTO chunks VALUES (1)")
        .execute(&mut conn)
        .await
        .unwrap();
    drop(conn);

    let before = std::fs::read(&db_path).unwrap();
    let err = match SqliteStore::open_at(&db_path).await {
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

#[tokio::test]
async fn opening_a_newer_database_is_refused_with_a_newer_asobi_error() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("from-the-future.db");
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
        .await
        .unwrap();
    sqlx::raw_sql("PRAGMA user_version = 10;")
        .execute(&mut conn)
        .await
        .unwrap();
    drop(conn);

    let before = std::fs::read(&db_path).unwrap();
    let err = match SqliteStore::open_at(&db_path).await {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected refusal"),
    };
    assert!(err.contains("newer"), "{err}");
    assert_eq!(before, std::fs::read(&db_path).unwrap());
}

#[tokio::test]
async fn a_new_database_is_created_directly_at_schema_nine() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");
    {
        let store = SqliteStore::open_at(&db_path).await.unwrap();
        store
            .create_entities(vec![EntityInput {
                name: "project:task".into(),
                entity_type: "task".into(),
                observations: vec![],
            }])
            .await
            .unwrap();
    }
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db_path.display()))
        .await
        .unwrap();
    // Version tracking moved to sqlx migrations (ADR 0008): a new file keeps
    // user_version 0 and records the baseline in _sqlx_migrations instead.
    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(user_version, 0);
    let migrations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(migrations, 1, "the baseline migration is applied");
    let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(
        auto_vacuum, 2,
        "a new database should switch on incremental auto-vacuum"
    );
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('asobi_entities')")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert!(columns.contains(&"last_activity".to_string()));
    assert!(!columns.contains(&"updated_at".to_string()));
}

#[tokio::test]
async fn applied_purge_reclaims_space_via_incremental_vacuum() {
    let (dir, store) = store().await;
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec!["done".into()],
        }])
        .await
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .await
        .unwrap();

    let db = dir.path().join("contract.db");
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    sqlx::raw_sql(
        "UPDATE asobi_entities SET created_at = datetime('now', '-90 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-90 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-90 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-90 days');",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    drop(conn);

    let report = store
        .purge(PurgeRequest {
            older_than_days: 30,
            apply: true,
        })
        .await
        .unwrap();
    assert_eq!(report.deleted, 1);

    // `incremental_vacuum` after an applied purge must not error even
    // though this store was opened fresh (auto_vacuum already INCREMENTAL).
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(auto_vacuum, 2);
}

/// `show` is the only eager read, and it was unbounded: loading a session at
/// the 200-observation cap cost tens of thousands of tokens to answer "where
/// was I". Context is the scarce resource, so it returns the current end of the
/// trail by default — while still reporting the true total, since a caller that
/// cannot tell what it is missing is worse off than one reading everything.
#[tokio::test]
async fn show_returns_recent_observations_and_the_true_total() {
    let (_dir, store) = store().await;
    shared_contract::show_returns_recent_observations_and_the_true_total(&store).await;
}

/// Retention has to happen without being asked. The previous design was a
/// manual purge that was correct in every respect except that it never ran:
/// six weeks of daily use left a graph that was 96% finished work.
///
/// It fires on the first *write* rather than at open, so a pure read never
/// mutates the graph — `asobi show` must not delete anything.
#[tokio::test]
async fn finished_work_is_swept_on_the_first_write_not_on_reads() {
    let (dir, store) = store().await;
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
        .await
        .unwrap();
    store
        .truth_upsert("project:task", "status", "DONE")
        .await
        .unwrap();

    let db = dir.path().join("contract.db");
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
        .await
        .unwrap();
    sqlx::raw_sql(
        "UPDATE asobi_entities SET created_at = datetime('now', '-30 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-30 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-30 days');",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    drop(conn);

    // A fresh handle, as a new process would have.
    let reopened = SqliteStore::open_at(&db).await.unwrap();
    assert_eq!(
        reopened.read_graph().await.unwrap().entities.len(),
        2,
        "a read must not sweep"
    );

    reopened
        .truth_upsert("project:concept", "note", "anything")
        .await
        .unwrap();
    let after = reopened.read_graph().await.unwrap();
    assert_eq!(after.entities.len(), 1, "the finished task is gone");
    assert_eq!(after.entities[0].name, "project:concept");
}

/// Truth values are searchable. The convention is to store a pitfall's
/// human-readable warning in a `title` truth so session start can surface it
/// cheaply — which made the one sentence explaining a dead end the one thing
/// recall could not reach.
#[tokio::test]
async fn search_reaches_truth_values_not_just_observations() {
    let (_dir, store) = store().await;
    shared_contract::search_reaches_truth_values_not_just_observations(&store).await;
}

/// A multi-word question must not fail closed. FTS5 ANDs bare terms, so a
/// natural-language query whose words are spread across an observation, a truth
/// and a name matched nothing — and an empty result is indistinguishable from
/// "nothing was ever recorded", which is the wrong way for a pitfall lookup to
/// fail.
#[tokio::test]
async fn search_widens_rather_than_returning_a_silent_zero() {
    let (_dir, store) = store().await;
    shared_contract::search_widens_rather_than_returning_a_silent_zero(&store).await;
}

/// Ranking survives to the caller. The entity fetch used `ORDER BY name`, which
/// re-sorted results alphabetically and silently discarded whatever ranking
/// search had computed — so relevance never reached the caller at all.
#[tokio::test]
async fn search_returns_results_in_ranked_order_not_alphabetical() {
    let (_dir, store) = store().await;
    shared_contract::search_returns_results_in_ranked_order_not_alphabetical(&store).await;
}

#[tokio::test]
async fn observations_move_last_activity_in_every_direction() {
    let (dir, store) = store().await;
    let db = dir.path().join("contract.db");
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();

    // Add moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store
        .add_observations(
            vec![ObservationInput {
                entity_name: "project:task".into(),
                contents: vec!["first note".into()],
            }],
            200,
        )
        .await
        .unwrap();
    let after_add = last_activity_of(&db, "project:task").await;
    assert!(
        after_add > aged,
        "an observation add must move last_activity"
    );

    // Edit moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store
        .update_observation("project:task", "first note", "edited note")
        .await
        .unwrap();
    let after_edit = last_activity_of(&db, "project:task").await;
    assert!(
        after_edit > aged,
        "an observation edit must move last_activity"
    );

    // Delete moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store
        .delete_observations(vec![ObservationDeletion {
            entity_name: "project:task".into(),
            observations: vec!["edited note".into()],
        }])
        .await
        .unwrap();
    let after_delete = last_activity_of(&db, "project:task").await;
    assert!(after_delete > aged);
}

#[tokio::test]
async fn truths_move_last_activity_in_every_direction() {
    let (dir, store) = store().await;
    let db = dir.path().join("contract.db");
    store
        .create_entities(vec![EntityInput {
            name: "project:task".into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();

    // Upsert (insert) moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store
        .truth_upsert("project:task", "status", "IN_PROGRESS")
        .await
        .unwrap();
    assert!(
        last_activity_of(&db, "project:task").await > aged,
        "a truth insert must move last_activity"
    );

    // Upsert (update) moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store
        .truth_upsert("project:task", "status", "DONE")
        .await
        .unwrap();
    assert!(
        last_activity_of(&db, "project:task").await > aged,
        "a truth update must move last_activity"
    );

    // Delete moves it.
    age_last_activity(&db, "project:task").await;
    let aged = last_activity_of(&db, "project:task").await;
    store.truth_delete("project:task", "status").await.unwrap();
    assert!(
        last_activity_of(&db, "project:task").await > aged,
        "a truth delete must move last_activity"
    );
}

#[tokio::test]
async fn relations_do_not_move_last_activity() {
    let (dir, store) = store().await;
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
        .await
        .unwrap();
    let before = last_activity_of(&db, "project:epic").await;

    let relation = RelationInput {
        from: "project:task".into(),
        to: "project:epic".into(),
        relation_type: "part_of".into(),
    };
    store
        .create_relations(vec![relation.clone()])
        .await
        .unwrap();
    assert_eq!(
        last_activity_of(&db, "project:epic").await,
        before,
        "relations are not activity"
    );

    store.delete_relations(vec![relation]).await.unwrap();
    assert_eq!(last_activity_of(&db, "project:epic").await, before);
}
