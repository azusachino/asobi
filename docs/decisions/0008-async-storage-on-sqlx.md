---
id: 0008
title: "0008. Async storage on sqlx"
date: 2026-09-27
status: proposed
tags: [storage, api, async, sqlx, v0.8]
supersedes: docs/decisions/0002-why-rusqlite.md
related: [0001-sqlite-only-v2-rewrite.md, 0005-remote-server.md, 0007-clean-schema-baseline.md]
---

## Context

[0001](0001-sqlite-only-v2-rewrite.md) collapsed storage onto synchronous `rusqlite` behind a synchronous `api::v2`, deleting an async surface (and an earlier sqlx dependency) that existed only for backends that never arrived. It anticipated that a server would "reintroduce concurrency at the process level".

0.8 adds that server ([0005](0005-remote-server.md)), and two things are now concrete rather than speculative:

- **The server is async.** It is built on tokio, axum and hyper, and serves several devices concurrently. Calling a synchronous store from it means `spawn_blocking` on every request and a single connection behind a mutex.
- **PostgreSQL is a real next step.** [0005](0005-remote-server.md)'s roadmap puts a PostgreSQL store behind the server. `rusqlite` is SQLite-only, so that store would share nothing with this one.

Options considered: keep `rusqlite` and block inside the server; an ORM (toasty, SeaORM, Diesel with `diesel-async`); or sqlx. Asobi's valuable storage code is SQL an ORM does not model: FTS5 with BM25 ranking, triggers, atomic claims, pragmas. Through an ORM it would all go through a raw-SQL escape hatch anyway, so an ORM adds modelling for the part that is already simple. sqlx is SQL-first, async, supports SQLite and PostgreSQL, and brings a connection pool and migrations. toasty was also too young to build on (0.x, a breaking release roughly every six weeks).

## Decision

### sqlx is the storage layer

- `SqliteStore` is reimplemented on sqlx's SQLite driver, which bundles SQLite (built with FTS5) and links it statically, as `rusqlite` did. `rusqlite` is removed.
- Queries are **runtime-checked** (`sqlx::query`, `query_as`), not the compile-time `query!` macros: asobi builds some SQL dynamically, and the build must not need a database or a generated query cache.
- One `SqlitePool` per graph file. Connection options set WAL, `synchronous = NORMAL`, foreign keys and the busy timeout; a new file gets `auto_vacuum = INCREMENTAL` before its first write. SQLite still has one writer: concurrency comes from WAL readers and the busy timeout, not from the pool.
- Transactions that must be atomic against other writers (task claims, abandonment) begin with `BEGIN IMMEDIATE` (`Connection::begin_with`), as they do today.

### An async `v3` API

- `api::v2` is deleted and `api::v3` replaces it: the same four capabilities (`GraphStore`, `SearchStore`, `MaintenanceStore`, `TaskStore`), the same types, with `async fn` methods whose futures are `Send` (required by axum's multi-threaded runtime). Per `api::v1`'s own rule, a breaking change is a new version, not a mutated old one.
- The CLI runs each command on a single-threaded tokio runtime. The server runs a multi-threaded runtime and calls the store directly, without `spawn_blocking`.
- `RemoteStore` ([0005](0005-remote-server.md)) implements `v3` over HTTP with reqwest, one client per process.

### sqlx migrations carry the schema

- The schema-9 baseline of [0007](0007-clean-schema-baseline.md) becomes the first sqlx migration; later changes are further migrations, tracked by sqlx (`_sqlx_migrations`), a mechanism PostgreSQL shares.
- The pre-0.8 refusal is unchanged in behaviour. Before the pool opens, a plain read-only connection reads `PRAGMA user_version`: a non-zero value means a file from an earlier Asobi, which is refused with the same move-aside error and left untouched. The check must precede the pool because the pool's connection options (`journal_mode`) write to the file.

### PostgreSQL stays out of 0.8

The code is organised so a `PgStore` can implement `v3` later, but 0.8 ships SQLite only. Search (FTS5/BM25 vs `tsvector`/`ts_rank`), activity triggers and claim locking differ per database and are written per backend when PostgreSQL arrives.

## Verification

- Every existing contract, CLI, sweep and schema test passes on the sqlx store, adapted only for `async`.
- A test confirms FTS5 search with BM25 ordering works on the bundled build.
- The concurrency test (many processes claiming tasks) passes: no task is claimed twice.
- The refusal test still proves a refused file is byte-for-byte unchanged.
- CLI start-up and a short command are timed against 0.7.3 with `hyperfine`; the numbers are recorded in the implementing PR.

## Consequences

- This supersedes [0002](0002-why-rusqlite.md) and reverses [0001](0001-sqlite-only-v2-rewrite.md)'s synchronous API. 0001's lesson stands: the async surface is justified by a server that exists and a PostgreSQL path on the roadmap, not by hypothetical backends.
- [0005](0005-remote-server.md)'s "one request at a time" becomes pooled concurrent requests, with SQLite serialising writes; its "no async runtime" constraint is withdrawn.
- [0007](0007-clean-schema-baseline.md)'s behaviour holds; only its versioning mechanism moves from `user_version` to sqlx migrations.
- Every CLI command starts a tokio runtime, and the binary grows. Both are measured, not assumed.
- The storage code written in WP2 and WP2b (abandonment, activity triggers, refusal) is ported, not redesigned.
