// storage-boundary: provider-test -- this bench inspects SQLite query plans directly.
use anyhow::Result;
use sqlx::Connection;
use sqlx::Row;
use sqlx::sqlite::SqliteConnectOptions;
use std::str::FromStr;
use tempfile::tempdir;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(run())
}

async fn run() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("plans.db");
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true);
    let mut conn = sqlx::SqliteConnection::connect_with(&options).await?;
    sqlx::raw_sql(include_str!("../migrations/0001_baseline.sql"))
        .execute(&mut conn)
        .await?;
    explain(
        &mut conn,
        "observation FTS",
        "SELECT o.entity_name FROM asobi_obs_fts JOIN asobi_observations o ON asobi_obs_fts.rowid=o.rowid WHERE asobi_obs_fts MATCH $1 ORDER BY bm25(asobi_obs_fts) LIMIT $2",
        &["commonterm", "10"],
    )
    .await?;
    explain(
        &mut conn,
        "truth lookup",
        "SELECT entity_name FROM asobi_truths WHERE key=$1 AND value=$2",
        &["status", "READY"],
    )
    .await?;
    explain(
        &mut conn,
        "relation lookup",
        "SELECT from_entity,to_entity FROM asobi_relations WHERE from_entity=$1 OR to_entity=$1",
        &["entity-1"],
    )
    .await?;
    Ok(())
}

async fn explain(
    conn: &mut sqlx::SqliteConnection,
    label: &str,
    sql: &'static str,
    values: &[&str],
) -> Result<()> {
    println!("\n[{label}]");
    let mut query = sqlx::query(sql);
    for value in values {
        query = query.bind(value);
    }
    for row in query.fetch_all(&mut *conn).await? {
        println!("{}", row.try_get::<String, _>(0)?);
    }
    Ok(())
}
