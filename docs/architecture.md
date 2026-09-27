# Asobi architecture

Asobi is a focused, single-command-at-a-time knowledge-graph CLI on an async core. `main.rs` is only the process entry point; command routing, API contracts, storage, tasks, and compaction live in their own modules.

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

The API is async (v3, Send futures on tokio) so the same store traits serve both the CLI's current-thread runtime and the server's multi-threaded runtime (ADR 0008). Each operation is still a short transaction, and SQLite's WAL mode lets readers proceed while a writer commits. The task dispatcher claims a READY task and records its claim observation in one immediate transaction.

## SQLite storage

`src/storage/sqlite.rs` owns schema creation, connection settings, and queries. There is no upgrade chain: a new database is created directly at schema 9, and a file from any earlier version is refused with a move-aside message (ADR 0007). The database uses foreign keys, WAL, bounded busy timeouts, and an external-content FTS5 index with porter stemming and BM25 ranking. Truth filters are applied in SQL and combine with keyword search through AND semantics.

## Durable projections

`compact` renders durable graph entities to Markdown topics. It is a deterministic graph-to-Markdown projection; it does not ingest documents or build embeddings. Sessions and tasks remain graph data, available through graph, search, and show.

## Verification

The quality gate combines the v2 backend contract tests, CLI integration tests, multi-process concurrency tests, benchmark compilation, formatting, linting, and the storage-boundary verifier. `make check` is the authoritative list; benchmark sources stay under `benches/` so hot paths can be measured as the implementation evolves.
