//! Regression guard for the `make bench` global-graph wipe (fixed in 15b051c).
//!
//! `benches/graph.rs` once set the wrong env var (`DATABASE_URL` instead of
//! `ASOBI_DATABASE_URL`), so `init_db` ignored it and the bench seeded and
//! `mcp_reset`'d the user's real global graph. This asserts the property that
//! makes the bench safe: pointing `ASOBI_DATABASE_URL` at a scratch file
//! fully isolates writes and resets from any other database file.

use asobi::backend::turso::db;
use asobi::model::EntityInput;
use tempfile::tempdir;

fn set_db(path: &std::path::Path) {
    unsafe { std::env::set_var(db::ENV_DATABASE_URL, path.to_str().unwrap()) };
}

async fn seed(conn: &turso::Connection, name: &str) {
    db::create_entities(
        conn,
        vec![EntityInput {
            name: name.to_string(),
            entity_type: "project".to_string(),
            observations: vec!["precious".to_string()],
        }],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn bench_env_var_isolates_real_graph() {
    let dir = tempdir().unwrap();
    let real_db = dir.path().join("real.db");
    let scratch_db = dir.path().join("scratch.db");

    // Seed the "real" graph.
    set_db(&real_db);
    {
        let (_db, conn) = db::init_db().await.unwrap();
        seed(&conn, "keep-me").await;
    }

    // Simulate the bench: redirect to a scratch DB, seed it, then reset it —
    // exactly the lifecycle benches/graph.rs runs per size.
    set_db(&scratch_db);
    {
        let (_db, conn) = db::init_db().await.unwrap();
        seed(&conn, "bench-entity").await;
        db::reset(&conn).await.unwrap();
    }

    // The real graph must survive the bench's reset untouched.
    set_db(&real_db);
    let (_db, conn) = db::init_db().await.unwrap();
    let graph = db::open_nodes(&conn, vec!["keep-me".to_string()])
        .await
        .unwrap();
    assert_eq!(graph.entities.len(), 1, "bench reset wiped the real graph");
    assert_eq!(graph.entities[0].name, "keep-me");
}
