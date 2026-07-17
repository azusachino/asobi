# Changelog

## v0.6.1 — Lean agent reads and safe retention

### Added

- Preview-first `purge` for stale terminal `session` and `task` entities, with transactional deletion, relation/FTS cleanup, JSON reports, and durable-knowledge safeguards.
- Shell completion generation for Bash, Elvish, Fish, PowerShell, and Zsh through `asobi completions <shell>`.

### Changed

- `graph` and `search` now return lean entity indexes: observations and skill bodies are lazy and available through explicit `show`, `export`, `backup`, or `skills show` operations.
- Updated the usage guide and retention ADR with the 0.6.1 maintenance and completion workflows.

### Verification

`make check` passes, including storage-boundary checks, Clippy, Rust tests, CLI integration checks, use cases, and benchmark compilation. Tarpaulin reports 62.60% line coverage (1,053/1,682).

## v0.6.0 — Curated SQLite graph storage

### Added

- Synchronous `api::v2` storage traits for graph, search, skills, snapshots, backups, maintenance, and task dispatch.
- Bundled SQLite through `rusqlite`, with WAL mode, foreign keys, bounded busy timeouts, and FTS5/BM25 keyword search.
- Durable task dispatcher: `tasks plan`, `list`, `dispatch`, `sync`, and `close` with nested help, lifecycle validation, and JSON response schemas.
- Atomic task dispatch: status transition, claimant truth, and dispatch observation commit together, so concurrent agents produce one winner.
- Graph-to-Markdown `compact` projection for durable knowledge topics.
- Contract, CLI, evil-input, edge-case, concurrent-process, daily-practice, and benchmark coverage.

### Removed

- libSQL/Turso and SQLx providers.
- Vector/document ingestion, semantic recall, and feature-gated product paths.
- The obsolete async v1 storage contract and provider-specific verification scripts.

### Verification

`make check` runs formatting, Clippy, all Rust tests, the CLI verifier, the daily use-case scenario, and storage-boundary checks. `cargo bench --no-run` verifies all benchmark targets; `make bench` executes them.

## v0.5.2 — Versioned CLI responses

- Added command-specific JSON Schema discovery through `schema` and `schema --command NAME`.
- Standardized structured errors and local-time tracing output.

## v0.5.1 — Leaner CLI build

- Reduced default CLI dependencies and tightened logging and formatting gates.

## v0.5.0 and earlier

- Established the standalone knowledge-graph CLI, SQLite-compatible graph schema, truths, observation history, lazy reads, skills, compact Markdown projections, portable JSON export/import, and local/XDG workspace layouts.
