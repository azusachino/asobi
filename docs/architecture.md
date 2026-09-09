# Asobi architecture

Asobi 0.7 is a focused, synchronous knowledge-graph CLI. `main.rs` is only the process entry point; command routing, API contracts, storage, tasks, skills, and compaction live in their own modules.

```text
CLI commands
    |
api::v2 capability traits
    |
storage::SqliteStore
    |
SQLite + WAL + FTS5
```

## API boundary

`src/api/v2.rs` contains domain requests, results, errors, and capability traits. It does not expose SQL statements, connection handles, or SQLite row types. The application composes `SqliteStore`, while commands depend only on the API traits.

The API is synchronous because this is a local SQLite CLI: each operation is a short transaction, and SQLite's WAL mode allows readers to proceed while a writer commits. The task dispatcher claims a READY task and records its claim observation in one immediate transaction.

## SQLite storage

`src/storage/sqlite.rs` owns schema creation, migrations, connection settings, and queries. The database uses foreign keys, WAL, bounded busy timeouts, and an external-content FTS5 index with porter stemming and BM25 ranking. Truth filters are applied in SQL and combine with keyword search through AND semantics.

## Durable projections

`compact` renders durable graph entities to Markdown topics. It is a deterministic graph-to-Markdown projection; it does not ingest documents or build embeddings. Sessions and tasks remain graph data, available through graph, search, and show. Skills are not graph data: since 0.7 they live on the filesystem under the skills directory, with a `.asobi-skills.json` manifest recording each one's source and commit.

## Verification

The quality gate combines the v2 backend contract tests, CLI integration tests, multi-process concurrency tests, benchmark compilation, formatting, linting, and the storage-boundary verifier. Benchmark sources remain under `benches/` so storage, graph, task, allocation, and SQL-plan behavior can be measured as the implementation evolves.
