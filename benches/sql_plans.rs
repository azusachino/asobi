use anyhow::Result;
use rusqlite::{Connection, params};
use tempfile::tempdir;

fn main() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("plans.db");
    let _store = asobi::storage::SqliteStore::open_at(&path)?;
    let conn = Connection::open(path)?;
    explain(
        &conn,
        "observation FTS",
        "EXPLAIN QUERY PLAN SELECT o.entity_name FROM asobi_obs_fts JOIN asobi_observations o ON asobi_obs_fts.rowid=o.rowid WHERE asobi_obs_fts MATCH ? ORDER BY bm25(asobi_obs_fts) LIMIT ?",
        params!["commonterm", 10_i64],
    )?;
    explain(
        &conn,
        "truth lookup",
        "EXPLAIN QUERY PLAN SELECT entity_name FROM asobi_truths WHERE key=? AND value=?",
        params!["status", "READY"],
    )?;
    explain(
        &conn,
        "relation lookup",
        "EXPLAIN QUERY PLAN SELECT from_entity,to_entity FROM asobi_relations WHERE from_entity=? OR to_entity=?",
        params!["entity-1", "entity-1"],
    )?;
    Ok(())
}

fn explain(conn: &Connection, label: &str, sql: &str, values: impl rusqlite::Params) -> Result<()> {
    println!("\n[{label}]");
    let mut stmt = conn.prepare(sql)?;
    for row in stmt.query_map(values, |r| r.get::<_, String>(3))? {
        println!("{}", row?);
    }
    Ok(())
}
