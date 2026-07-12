// storage-boundary: provider-test
use anyhow::Result;
use libsql::{Connection, Value, params_from_iter};
use tempfile::tempdir;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let dir = tempdir()?;
        let db_path = dir.path().join("plans.db");
        unsafe {
            std::env::set_var(
                asobi::paths::ENV_DATABASE_URL,
                db_path.to_str().expect("utf-8 path"),
            );
        }
        let (_db, conn) = asobi::storage::libsql::db::init_db().await?;

        explain(
            &conn,
            "observation FTS",
            "EXPLAIN QUERY PLAN SELECT o.entity_name FROM asobi_obs_fts \
             JOIN asobi_observations o ON asobi_obs_fts.rowid = o.rowid \
             WHERE asobi_obs_fts MATCH ?1 ORDER BY bm25(asobi_obs_fts) LIMIT ?2",
            vec![Value::from("commonterm"), Value::from(10_i64)],
        )
        .await?;
        explain(
            &conn,
            "entity name fallback",
            "EXPLAIN QUERY PLAN SELECT name FROM asobi_entities \
             WHERE name LIKE ?1 OR entity_type LIKE ?1 ORDER BY name LIMIT ?2",
            vec![Value::from("%commonterm%"), Value::from(10_i64)],
        )
        .await?;
        explain(
            &conn,
            "truth filter",
            "EXPLAIN QUERY PLAN SELECT entity_name FROM asobi_truths \
             WHERE key = ?1 AND value = ?2 GROUP BY entity_name \
             HAVING COUNT(DISTINCT key) = ?3",
            vec![
                Value::from("status"),
                Value::from("READY"),
                Value::from(1_i64),
            ],
        )
        .await?;
        explain(
            &conn,
            "relation neighborhood",
            "EXPLAIN QUERY PLAN SELECT from_entity, to_entity, relation_type \
             FROM asobi_relations WHERE from_entity = ?1 OR to_entity = ?1",
            vec![Value::from("entity-1")],
        )
        .await?;

        let mut rows = conn.query("PRAGMA index_list('asobi_truths')", ()).await?;
        println!("\n[truth indexes]");
        while let Some(row) = rows.next().await? {
            println!("{}", row.get::<String>(1)?);
        }
        Ok::<(), anyhow::Error>(())
    })
}

async fn explain(conn: &Connection, label: &str, sql: &str, values: Vec<Value>) -> Result<()> {
    println!("\n[{label}]");
    let mut rows = conn.query(sql, params_from_iter(values)).await?;
    while let Some(row) = rows.next().await? {
        println!("{}", row.get::<String>(3)?);
    }
    Ok(())
}
