use std::path::Path;

use tempfile::tempdir;
use turso::{Builder, Value};

async fn open_turso_db(path: &Path) -> (turso::Database, turso::Connection) {
    let db = Builder::new_local(path.to_str().unwrap())
        .experimental_multiprocess_wal(true)
        .experimental_index_method(true)
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    (db, conn)
}

#[tokio::test]
async fn turso_graph_foundations_round_trip() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("foundations.db");
    let (_db, conn) = open_turso_db(&db_path).await;

    conn.execute("PRAGMA foreign_keys = ON", ()).await.unwrap();
    conn.execute(
        "CREATE TABLE entities (name TEXT PRIMARY KEY, kind TEXT NOT NULL)",
        (),
    )
    .await
    .unwrap();
    conn.execute(
        "CREATE TABLE observations (
            id INTEGER PRIMARY KEY,
            entity_name TEXT NOT NULL REFERENCES entities(name) ON DELETE CASCADE,
            content TEXT NOT NULL
        )",
        (),
    )
    .await
    .unwrap();
    conn.execute(
        "CREATE TABLE truths (
            entity_name TEXT NOT NULL REFERENCES entities(name) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            PRIMARY KEY (entity_name, key)
        )",
        (),
    )
    .await
    .unwrap();

    conn.execute("BEGIN IMMEDIATE", ()).await.unwrap();
    conn.execute(
        "INSERT INTO entities (name, kind) VALUES (?1, ?2)",
        ("example:entity", "concept"),
    )
    .await
    .unwrap();
    conn.execute(
        "INSERT INTO observations (id, entity_name, content) VALUES (?1, ?2, ?3)",
        (1_i64, "example:entity", "first observation"),
    )
    .await
    .unwrap();
    conn.execute(
        "INSERT INTO truths (entity_name, key, value) VALUES (?1, ?2, ?3)",
        ("example:entity", "status", "READY"),
    )
    .await
    .unwrap();
    conn.execute("COMMIT", ()).await.unwrap();

    let mut rows = conn
        .query(
            "SELECT name, kind FROM entities WHERE name = ?1",
            ("example:entity",),
        )
        .await
        .unwrap();
    let row = rows.next().await.unwrap().unwrap();
    assert_eq!(row.get::<String>(0).unwrap(), "example:entity");
    assert_eq!(row.get::<String>(1).unwrap(), "concept");

    let mut dynamic_params = vec![Value::from("example:entity"), Value::from("status")];
    dynamic_params.push(Value::from("READY"));
    let mut rows = conn
        .query(
            "SELECT value FROM truths WHERE entity_name = ?1 AND key = ?2 AND value = ?3",
            turso::params_from_iter(dynamic_params),
        )
        .await
        .unwrap();
    assert_eq!(
        rows.next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "READY"
    );

    conn.execute("DELETE FROM entities WHERE name = ?1", ("example:entity",))
        .await
        .unwrap();
    let mut rows = conn
        .query("SELECT COUNT(*) FROM observations", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        0
    );
    let mut rows = conn.query("SELECT COUNT(*) FROM truths", ()).await.unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        0
    );
}

#[tokio::test]
async fn turso_transaction_rolls_back() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("rollback.db");
    let (_db, conn) = open_turso_db(&db_path).await;

    conn.execute("CREATE TABLE values_table (value TEXT NOT NULL)", ())
        .await
        .unwrap();
    conn.execute("BEGIN IMMEDIATE", ()).await.unwrap();
    conn.execute(
        "INSERT INTO values_table (value) VALUES (?1)",
        ("discarded",),
    )
    .await
    .unwrap();
    conn.execute("ROLLBACK", ()).await.unwrap();

    let mut rows = conn
        .query("SELECT COUNT(*) FROM values_table", ())
        .await
        .unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
        0
    );
}
