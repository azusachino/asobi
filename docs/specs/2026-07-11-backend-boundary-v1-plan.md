# Asobi backend boundary v1

**Date:** 2026-07-11
**Status:** DRAFT — requires review before implementation
**Epic:** `asobi:backend-boundary-v1`

## Objective

Make libSQL the supported default backend and retain Turso as an explicit,
feature-gated experimental backend without allowing database drivers, SQL,
connection types, schema details, or database filenames to escape `src/backend/`.

This plan preserves the current compact contract. Compact remains a one-way
projection from graph state to Markdown and the document index. Cross-device
continuation uses explicit backend-neutral `export` and `import`; it does not
copy a database file and does not turn compact into synchronization.

## Why v1

The v1 API has not been released or delivered as a stable public contract. Its
current surface is not sufficient for the application workflows that still call
a concrete backend directly: skills, logical snapshots, local backup, and
document maintenance. Correct v1 in place now, before it becomes a compatibility
promise.

## Target layering

```text
CLI
  -> application commands
      -> api::v1 traits and request/result types
          -> backend::libsql | backend::turso (experimental)
```

- `application/` owns command orchestration, frontmatter, Git operations,
  Markdown rendering, embedding orchestration, and export-file handling.
- `api::v1` owns only versioned domain requests/results, stable errors, and
  storage capability traits.
- `backend/` owns drivers, SQL, schema, connection lifecycle, transactions,
  retries, and the backend's default state-file name.
- `main` parses arguments, asks the backend factory for an API façade, and
  calls application commands. It imports no driver and names no backend file.

## v1 contract

The core traits stay deliberately small. Each accepts domain request/result
types; none returns a row, connection, SQL value, filesystem database handle,
or driver error.

| Trait | Required responsibility |
| --- | --- |
| `GraphStore` | entities, observations, truths, relations, graph reads |
| `SearchStore` | graph keyword search and truth filtering |
| `DocumentStore` | topics, chunks, and document search |
| `SkillStore` | atomic skill record persistence, body lookup, source listing/removal |
| `SnapshotStore` | logical, backend-neutral graph export/import |
| `DocumentMaintenanceStore` | document-index inspection needed by compact, such as duplicate clusters |
| `MaintenanceStore` | health, stats, reset, and `BackendInfo` |

`BackupStore` is an optional capability separate from `SnapshotStore`:
logical snapshots are portable between backends; physical backups are not.
An unsupported backend returns the stable `Unsupported` error.

`BackendInfo` contains the backend identifier, API version, schema version,
state kind, and a structured capability record. Capabilities describe optional
behavior; application code must not probe by calling a concrete method.

## Backend and state selection

libSQL is the default provider. It resolves its own default local state as
`asobi.db`. Turso is compiled only with `turso-experimental` and resolves its
own independent state as `asobi.turso.db`. Those names belong in their backend
modules, not in paths or CLI code.

`ASOBI_DATABASE_URL` is an explicit override for the selected provider. A
backend factory is the only place that maps a selected backend to an API façade.
Default builds contain no Turso implementation; experimental builds expose
Turso only through the factory's explicit selection path.

## Portable handoff

`export` serializes a logical `Snapshot` with `apiVersion`, snapshot-format
version, and source backend metadata. `import` validates the format and writes
the same domain data into the selected backend. The intended Git workflow is:

```text
asobi export --scope <epic> --output .asobi/handoffs/<epic>.json
git commit …
git pull
asobi import .asobi/handoffs/<epic>.json
```

Compact is unchanged. It is neither a source of truth nor an importer.

## Work items

| # | Task | Status | Depends on |
| --- | --- | --- | --- |
| 1 | Correct and test the stable `api::v1` contract | DONE | — |
| 2 | Create application services and remove frontend driver references | REVIEW | 3 |
| 3 | Implement the libSQL v1 provider and default factory | AWAITING_VERIFY | 1 |
| 4 | Add the feature-gated Turso experimental provider | BLOCKED_ON task-2 | 2 |
| 5 | Add shared contracts plus backend-specific verify and benchmark gates | BLOCKED_ON task-2,task-3,task-4 | 2,3,4 |
| 6 | Document state isolation, handoff, and experimental support | BLOCKED_ON task-5 | 5 |

### Task 1 — correct and test `api::v1`

Add the missing v1 domain models, capability traits, stable errors, and a
compile-time contract suite. Add only capabilities with a current
application consumer; avoid generic database helpers. Done when a fake backend
can satisfy the contract without importing a driver and the public API has no
SQL- or driver-shaped types.

### Task 2 — application services

Split command workflows from storage implementation. `skills` keeps Git and
frontmatter behavior but uses `SkillStore`; `compact` keeps its current Markdown
semantics but uses graph/document traits; backup and export/import use their
respective v2 traits. `main` only invokes application services. Done when a
boundary check rejects backend imports outside `src/backend/`.

### Task 3 — libSQL default

Implement all v2 traits with the existing libSQL 0.9 backend and introduce the
default factory. Move libSQL state-file resolution into this provider. Preserve
all existing supported behavior with libSQL-specific regression tests for FTS5
stemming, BM25, vector operations, local backup, and concurrent writes.

### Task 4 — experimental Turso

Make `turso-experimental` an optional Cargo feature. Compile Turso only under
that feature, implement the same v2 contract, and resolve its own independent
state file inside the provider. Differences from libSQL must be surfaced in
`BackendInfo` capabilities and documented as experimental behavior.

### Task 5 — validation matrix

Add one generic v2 contract suite run against every compiled backend, plus
backend-specific suites. Add `verify-libsql`, `verify-turso`, `bench-libsql`,
and `bench-turso` Make targets. Default CI runs libSQL; a separate experimental
matrix enables Turso. The boundary verifier is a required gate.

### Task 6 — user-facing contract

Update README, architecture, usage, changelog, and Makefile help together.
Document default libSQL state, experimental Turso state, explicit export/import
handoff, and compact's unchanged one-way Markdown role.

## Non-goals

- No automatic multi-device synchronization protocol. That would require a
  separate cursor/event/conflict design.
- No committing database files, cache directories, or physical backups.
- No runtime alias that maps Turso names to libSQL or vice versa.
- No change to compact's durable-knowledge filtering or Markdown format.

## Acceptance

- No driver or backend module reference outside `src/backend/` and designated
  backend-specific tests.
- Default build, tests, verification, and benchmarks use libSQL only.
- Turso is absent from default builds and exercised only by its feature-gated
  matrix.
- A scoped export imports successfully into either backend without sharing a
  database file.
- Compact behavior is unchanged.
