// storage-boundary: provider-test
//!
//! Abandonment sweep (ADR 0006 / WP2). These tests seed timestamps directly in
//! SQLite, which is provider detail — hence the marker above.
//!
//! Every test pins both sweep windows through the environment (the first link
//! in the resolution chain) so neither the repo's `asobi.toml` nor the defaults
//! can shift a boundary under the assertions. Tests run serially under
//! `make check` (`--test-threads=1`) because the variables are process-global.

use asobi_core::api::{GraphStore, OpenNodes};
use asobi_core::model::{EntityInput, RelationInput};
use asobi_storage::SqliteStore;
use sqlx::Connection;
use sqlx::sqlite::SqliteConnection;
use tempfile::tempdir;

/// Set one of the sweep env vars for the body of a test and remove it on drop.
/// Removal (rather than restore) is the neutral state: no other test in this
/// suite sets these variables, and the suite runs serially.
struct EnvGuard {
    key: &'static str,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        unsafe { std::env::set_var(key, value) };
        EnvGuard { key }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(self.key) };
    }
}

async fn store() -> (tempfile::TempDir, std::path::PathBuf, SqliteStore) {
    let dir = tempdir().unwrap();
    let db = dir.path().join("sweep.db");
    let store = SqliteStore::open_at(&db).await.unwrap();
    (dir, db, store)
}

async fn task(store: &SqliteStore, name: &str, status: Option<&str>) {
    store
        .create_entities(vec![EntityInput {
            name: name.into(),
            entity_type: "task".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    if let Some(status) = status {
        store.truth_upsert(name, "status", status).await.unwrap();
    }
}

/// Age one entity's whole activity trail by `days`. This is the same MAX over
/// creation, observations and truth updates the sweep reads.
async fn age(db: &std::path::Path, name: &str, days: i64) {
    let mut conn = SqliteConnection::connect(&format!("sqlite://{}?mode=rw", db.display()))
        .await
        .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "UPDATE asobi_entities SET created_at = datetime('now', '-{days} days') WHERE name = '{name}';
         UPDATE asobi_observations SET created_at = datetime('now', '-{days} days') WHERE entity_name = '{name}';
         UPDATE asobi_truths SET updated_at = datetime('now', '-{days} days') WHERE entity_name = '{name}';
         -- Last: the observation/truth updates above fire the activity
         -- triggers, which would otherwise reset last_activity to now.
         UPDATE asobi_entities SET last_activity = datetime('now', '-{days} days') WHERE name = '{name}';"
    )))
    .execute(&mut conn)
    .await
    .unwrap();
}

async fn entity_of(store: &SqliteStore, name: &str) -> Option<asobi_core::model::EntityOutput> {
    store
        .open_nodes(OpenNodes {
            observation_limit: 0,
            names: vec![name.into()],
            ..Default::default()
        })
        .await
        .unwrap()
        .entities
        .into_iter()
        .next()
}

async fn status_of(store: &SqliteStore, name: &str) -> Option<String> {
    entity_of(store, name)
        .await
        .and_then(|e| e.truths.get("status").cloned())
}

#[tokio::test]
async fn idle_open_task_is_abandoned_with_an_observation_and_active_task_untouched() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "90");
    let (_dir, db, store) = store().await;
    task(&store, "proj:task-idle", Some("IN_PROGRESS")).await;
    task(&store, "proj:task-active", Some("IN_PROGRESS")).await;
    age(&db, "proj:task-idle", 30).await;

    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    assert_eq!(
        status_of(&store, "proj:task-idle").await.as_deref(),
        Some("ABANDONED")
    );
    let observations = entity_of(&store, "proj:task-idle")
        .await
        .unwrap()
        .observations;
    assert!(
        observations
            .iter()
            .any(|o| o.contains("abandoned automatically after 7 idle days")),
        "the abandonment observation must be on the trail, got {observations:?}"
    );
    // Active within the window: untouched.
    assert_eq!(
        status_of(&store, "proj:task-active").await.as_deref(),
        Some("IN_PROGRESS")
    );
}

#[tokio::test]
async fn statusless_idle_task_is_abandoned() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "90");
    let (_dir, db, store) = store().await;
    task(&store, "proj:task-ghost", None).await;
    age(&db, "proj:task-ghost", 30).await;

    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    assert_eq!(
        status_of(&store, "proj:task-ghost").await.as_deref(),
        Some("ABANDONED")
    );
}

#[tokio::test]
async fn epic_with_open_child_survives_but_is_abandoned_once_children_finish() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "90");
    let (_dir, db, store) = store().await;
    task(&store, "proj:epic", None).await;
    task(&store, "proj:epic:task-1", Some("IN_PROGRESS")).await;
    store
        .create_relations(vec![RelationInput {
            from: "proj:epic:task-1".into(),
            to: "proj:epic".into(),
            relation_type: "part_of".into(),
        }])
        .await
        .unwrap();
    age(&db, "proj:epic", 30).await;
    age(&db, "proj:epic:task-1", 30).await;

    // Both are idle and open, so both are candidates individually — but the
    // epic has an open `part_of` child, and the child has none. Exactly one
    // abandonment: the child. The epic's own idleness does not condemn it
    // while its child is being worked.
    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    assert_eq!(status_of(&store, "proj:epic").await.as_deref(), None);
    assert_eq!(
        status_of(&store, "proj:epic:task-1").await.as_deref(),
        Some("ABANDONED")
    );

    // Every child is now terminal, and the epic itself is still idle past the
    // window: now it goes.
    age(&db, "proj:epic", 30).await;
    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    assert_eq!(
        status_of(&store, "proj:epic").await.as_deref(),
        Some("ABANDONED")
    );
}

#[tokio::test]
async fn zero_abandon_days_disables_abandonment_but_retention_still_runs() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "0");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "7");
    let (_dir, db, store) = store().await;
    task(&store, "proj:task-open", Some("IN_PROGRESS")).await;
    task(&store, "proj:task-done", Some("DONE")).await;
    age(&db, "proj:task-open", 30).await;
    age(&db, "proj:task-done", 30).await;

    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 0, "0 disables abandonment");
    assert_eq!(
        status_of(&store, "proj:task-open").await.as_deref(),
        Some("IN_PROGRESS")
    );
    // Retention is a separate window and still ran: the finished task is gone.
    assert_eq!(report.purged, 1);
    assert!(status_of(&store, "proj:task-done").await.is_none());
}

#[tokio::test]
async fn abandoned_task_survives_retention_then_is_deleted_once_it_idles_out() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "7");
    let (_dir, db, store) = store().await;
    task(&store, "proj:task-late", Some("IN_PROGRESS")).await;
    age(&db, "proj:task-late", 8).await;

    // First sweep: abandoned — and the abandonment observation refreshes
    // last_activity, so retention must NOT delete it in the same pass.
    let report = store.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    assert_eq!(report.purged, 0, "a just-abandoned task is not deleted");
    assert_eq!(
        status_of(&store, "proj:task-late").await.as_deref(),
        Some("ABANDONED")
    );

    // Second lifecycle: once the abandonment itself goes quiet past the
    // retention window, the terminal task is deleted.
    age(&db, "proj:task-late", 30).await;
    let report = store.sweep().await.unwrap();
    assert_eq!(report.purged, 1);
    assert!(status_of(&store, "proj:task-late").await.is_none());
}

#[tokio::test]
async fn session_entities_are_no_longer_purged_by_retention() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "7");
    let (_dir, db, store) = store().await;
    store
        .create_entities(vec![EntityInput {
            name: "proj:session".into(),
            entity_type: "session".into(),
            observations: vec![],
        }])
        .await
        .unwrap();
    store
        .truth_upsert("proj:session", "status", "DONE")
        .await
        .unwrap();
    age(&db, "proj:session", 90).await;

    let report = store.sweep().await.unwrap();
    assert_eq!(report.purged, 0, "sessions are ordinary entities now");
    assert_eq!(
        status_of(&store, "proj:session").await.as_deref(),
        Some("DONE")
    );
}

#[tokio::test]
async fn direct_sweep_on_a_fresh_store_reports_its_own_work_once() {
    // A server calls `sweep()` on a store before any other write. The sweep's
    // own writes must not start a second, nested sweep that does the work and
    // leaves the outer call reporting nothing.
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "7");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "90");
    let (_dir, db, seeding) = store().await;
    task(&seeding, "proj:task-idle", Some("IN_PROGRESS")).await;
    drop(seeding);
    age(&db, "proj:task-idle", 30).await;

    let fresh = SqliteStore::open_at(&db).await.unwrap();
    let report = fresh.sweep().await.unwrap();
    assert_eq!(report.abandoned, 1);
    let observations = entity_of(&fresh, "proj:task-idle")
        .await
        .unwrap()
        .observations;
    assert_eq!(
        observations
            .iter()
            .filter(|o| o.contains("abandoned automatically"))
            .count(),
        1
    );
}
