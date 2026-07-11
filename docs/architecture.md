# Asobi architecture

Asobi has a versioned, backend-neutral API above a concrete storage backend.
The current backend is Turso; PostgreSQL and RocksDB remain design targets, not
supported runtime backends.

```text
CLI / document workflows
          |
       api::v1
  GraphStore  SearchStore  DocumentStore  MaintenanceStore  SnapshotStore
          |
   backend::turso::TursoBackend
          |
  Turso schema, native FTS, vector32, WAL/retry, and SQL
```

## API boundary

`src/api/v1.rs` contains only domain requests, results, capabilities, snapshots,
and stable errors. It does not import Turso, SQL, rows, pragmas, or connection
handles. The CLI receives a `TursoBackend` at startup and calls the capability
traits; a future backend can satisfy the same contract without changing the
domain-facing operations.

The API contract is versioned independently from the storage schema. Optional
features are reported through `BackendCapabilities`, and unsupported operations
return `ApiError::Unsupported` rather than silently doing nothing.

## Turso backend

All Turso-specific code lives under `src/backend/turso/`:

- `db.rs` — graph schema, CRUD, search, and migrations;
- `constant.rs` — Turso-owned SQL and schema statements;
- `vector.rs` — optional `vector32` storage and exact cosine search;
- `tx.rs` — multi-process WAL startup and immediate-transaction retries.

Graph keyword search uses Turso native `USING fts` indexes with `fts_match` and
`fts_score`. It does not use SQLite FTS5 virtual tables, external-content
triggers, porter stemming, or `bm25()`.

The document tier stores embeddings with `vector32` and searches with
`vector_distance_cos` over the chunks table. This is exact/brute-force search,
which is appropriate for the current project-document scale; no
`libsql_vector_idx` or ANN index is assumed.

Turso's experimental multi-process WAL is enabled at open. Database opening and
`BEGIN IMMEDIATE` writes retry transient lock contention with bounded backoff.
Legacy journal-mode and busy-timeout environment overrides are not supported.

## Database initialization

v0.5 initializes `data/asobi.turso.db` with the current schema, native FTS
indexes, and optional vector tables. A future file-backed backend should use a
backend-qualified filename of its own; remote backends do not need a local DB
file. If an old `asobi.db` is present while the Turso file is absent, startup
warns and still creates a fresh Turso database.

## Verification

The backend contract test checks that `TursoBackend` implements `api::v1::Backend`
and reports its capabilities. The real CLI checks are split into:

- `scripts/verify_cli.py` — backend-neutral graph behavior;
- `scripts/verify_cli.turso.py` — Turso backend identity/capabilities and native
  keyword search;
- `scripts/verify_documents_cli.py` — optional document workflow.

The first alternate backend should add its own backend-specific verifier while
reusing the API contract tests and backend-neutral CLI checks.
