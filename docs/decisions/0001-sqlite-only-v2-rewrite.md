---
id: 0001
title: "0001. Collapse to a single SQLite backend behind api::v2"
date: 2026-07-17
status: accepted
tags: [storage, api, rewrite, v0.6]
supersedes: docs/specs/2026-07-11-backend-api-v1.md, docs/specs/2026-07-11-backend-boundary-v1-plan.md, docs/specs/2026-07-11-storage-composition-adr.md, docs/specs/2026-07-17-sqlite-boundary.md, docs/specs/2026-07-17-v0.6-backend-design.md
pr: 24
---

## Context

Asobi 0.5.x targeted a pluggable, multi-backend storage layer behind an async `api::v1` contract (`src/api/v1.rs`, now deleted): `turso`/`libsql` was the shipped backend, with `postgres`/`rocksdb` planned as alternates. `api::v1`'s own versioning rule was explicit: _"v1 is frozen once the first alternate backend contract test passes. A breaking change creates v2; it does not add a required method to v1."_

In practice no second backend ever shipped, and the async trait surface (`#![allow(async_fn_in_trait)]`, `!Send` futures) existed only to accommodate backends that never arrived. It added real cost: two parallel backend trees (`src/storage/libsql/`, `src/storage/turso/`, ~4,700 deleted lines total), document/embedding/vector features (`src/recall.rs`, `src/ingest.rs`, `src/embed/`, `src/chunk.rs`) bolted onto the same boundary, and an async CLI that gained nothing from being async — every command is a single short-lived process making one local SQLite call.

## Decision

Rewrite storage around bundled `rusqlite` as the only backend, and freeze a new, synchronous `api::v2` (`src/api/v2.rs`) as the sole public storage contract, per `api::v1`'s own rule that a breaking change gets a new version rather than a mutated old one. `api::v1` is deleted rather than kept alongside `v2` — there is no second implementation of it and no external compatibility promise to preserve.

`api::v2` is deliberately narrower than `v1` was scoped to become: graph, FTS5 keyword search, skills, logical snapshot export/import, physical backup/restore, maintenance, and task dispatch. Document ingestion, embeddings, and vector search are out of the product boundary entirely, not just deferred to a future backend — they are not part of the 0.6 release build or verification matrix. See [0002](0002-why-rusqlite.md) for the backend choice itself.

The CLI (`src/main.rs`) was split into `src/cli/{commands,dispatch,graph, output,runtime,skills}.rs`; commands depend only on `api::v2` traits, never on `rusqlite` types directly (`src/storage/sqlite.rs` owns all SQL, schema, and pragmas).

Backup/restore were hardened as part of this same rewrite (post-review fixes, same PR): `restore` now checks `PRAGMA user_version` and required table presence before accepting a source file, checkpoints and removes `-wal`/`-shm` sidecars around the swap, and managed backups (no `--output`) are timestamped under `backups/` and pruned to `--keep` (explicit `--output` destinations are caller-owned and not pruned).

## Consequences

- Every storage call is now synchronous and blocking; this is correct for a short-lived CLI process and removes the `!Send` async ceremony, but any future long-lived server/daemon mode would need to reintroduce concurrency at the process level (e.g. one connection per request), not resurrect async traits.
- There is no supported migration path from a 0.5.x `turso`/`libsql` database to 0.6 — `asobi export`/`import` (logical JSON) is the only documented crossing point; there is no physical backend converter.
- Document ingestion, embeddings, and semantic recall are gone, not hidden behind a feature flag pending a future backend. Reintroducing them is a new product decision, not a bug.
- `api::v1` is gone from the tree; anyone referencing it in old forks/PRs must rebase onto `api::v2`. There is no `v1`/`v2` coexistence period.
