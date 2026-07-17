---
id: 0002
title: "0002. Backend comparison and why rusqlite"
date: 2026-07-17
status: accepted
tags: [storage, backend, rusqlite, v0.6]
related: [0001-sqlite-only-v2-rewrite.md]
pr: 24
---

## Context

Asobi is a single-user, local-first CLI: one short-lived process, one on-disk graph, no server, no concurrent-tenant story beyond a handful of cooperating agent processes on the same machine. Three storage approaches were built and shipped at various points before 0.6, then removed (see `CHANGELOG.md`, "Removed: libSQL/Turso and SQLx providers"):

- **`libsql`** — Turso's fork of SQLite with embedded-replica sync, an async driver, and vector-search extensions.
- **`turso` (hosted)** — the same engine reachable over the network as a managed remote database, with a local embedded replica.
- **`sqlx`** — an async, multi-database driver layer (Postgres/MySQL/SQLite) with compile-time query checking against a `DATABASE_URL`.
- **`postgres`** (considered, never implemented as a shipped provider) — a real client/server RDBMS.

## Comparison

| Backend | Deployment | Concurrency model | Fit for a local single-user CLI |
| --- | --- | --- | --- |
| `libsql`/`turso` | Embedded replica + optional remote sync | Async, `!Send` futures to support the replica sync task | Sync machinery (`src/storage/libsql/`, `src/storage/turso/`, ~4,700 lines) existed only for a replication feature the product never used — every asobi database is single-writer, single-machine |
| Turso (hosted) | Remote server | Network round-trip per operation | Requires being online and a hosted account for a tool whose entire value proposition is a local, always-available graph; adds latency to every CLI invocation |
| `sqlx` | In-process, any of several DBs | Async, needs `DATABASE_URL` at _build_ time for macro query-checking (or an offline query cache to maintain) | Fights a portable single binary: the state file's location is a runtime concern (`ASOBI_DATABASE_URL`, XDG dirs), not knowable at compile time; buys database portability Asobi doesn't need, since it only ever targets SQLite |
| `postgres` | Client/server | Async, connection pool | Requires an installed, running server process; contradicts "no setup, one binary, one file" |
| **`rusqlite`** | Bundled `libsqlite3`, in-process | Synchronous | One dependency, no server, no network, no build-time `DATABASE_URL`; matches the actual concurrency need (a `Mutex<Connection>` per process, WAL + `busy_timeout` across processes) |

## Decision

Standardize on `rusqlite` with the bundled SQLite feature as the only storage backend. `src/storage/sqlite.rs` opens one `Connection` per process, wrapped in a `Mutex`, configured with `PRAGMA journal_mode=WAL`, `synchronous=NORMAL`, `foreign_keys=ON`, and a busy timeout — this is enough concurrency control for cooperating local agent processes without an async runtime, a connection pool, or a replica-sync task.

FTS5 (bundled with `rusqlite`'s `bundled` feature) supplies keyword search directly; no separate embedding/vector store is needed for the product's scope (see [0001](0001-sqlite-only-v2-rewrite.md)).

## Consequences

- No async runtime dependency anywhere in the binary; the CLI's startup and per-command cost is dominated by process spawn, not runtime init.
- No network dependency and no remote account requirement — the tool works fully offline, which was the actual goal `turso`'s replica sync was trying (indirectly) to serve.
- Losing `sqlx`'s multi-database portability is accepted: Asobi has never shipped a non-SQLite backend, and the `api::v2` boundary ( [0001](0001-sqlite-only-v2-rewrite.md)) is what would carry a future backend, not the SQL driver choice.
- Scaling beyond a single local machine (true multi-writer, networked agents) is out of scope for this decision; it would need a new ADR, not a reversion to `turso`/`libsql` — the embedded-replica model was evaluated and rejected here specifically because it added complexity without matching Asobi's single-machine usage.
