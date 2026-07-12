# Changelog

## v0.5.2 — Versioned CLI Responses

### Breaking / Upgrade

- Existing JSON graph and mutation payloads remain unchanged.
- Use `asobi schema` or `asobi schema --command NAME` to discover and validate
  command-specific JSON Schemas.
- The schema version is independent from storage/export `apiVersion`; the
  initial schema version is `1`.

### Added

- JSON Schema derives for response data and a schema-validation gate in the
  CLI integration verifier.
- Consistent unpadded local-time log formatting.

## v0.5.1 — Local-Time Logs & Leaner Builds

### Fixed
- Log timestamps now render in the machine's local timezone instead of UTC, reusing the existing `chrono` dependency (no new crates, and avoiding tracing-subscriber's unsound `local-time` feature).

### Changed
- Trimmed the `documents` feature footprint: `fastembed` is pinned to text-only models over rustls/ureq, dropping the image-codec and reqwest/hyper/native-tls subtrees (~115 fewer crates).
- Trimmed the default build: `clap` drops its color and suggestion stack (7 fewer crates).

## v0.5.0 — Storage Backends, Scoped Export, and Better Recall

### Highlights

- **Scoped export:** `export --scope <entity>` creates a portable bundle for one epic and its task subtree. Add `--rationale` to include its cited decision chain.
- **Backend boundary:** the CLI uses a versioned storage API. libSQL is the supported default; Turso is an experimental opt-in.
- **Better recall:** the document tier uses quantized `gte-base-en-v1.5` embeddings.
- **Smaller build:** the default libSQL dependency contains only its local embedded core.
- **Performance tooling:** Criterion baselines, DHAT allocation profiles, and SQL query-plan reports cover graph and vector hot paths.
- **Truth history:** overwriting a truth records the superseded value with its valid-time window; read it with `asobi history <entity> [key]`.

### Current behavior

- libSQL is the default backend with ranked, stemmed FTS5 search and physical backup/restore.
- Turso is selected with `--features turso-experimental` and `ASOBI_BACKEND=turso`. It uses substring search, an isolated `asobi.turso.db`, and no physical backup/restore.
- JSON `export`/`import` is the portable handoff format across machines and backends.
- Physical restore validates the snapshot, saves the current database, closes live handles, atomically replaces the file, and clears stale WAL sidecars.
- Storage manages concurrency settings; legacy journal-mode and busy-timeout overrides are unavailable.
- Truth filters and reverse relation lookups use covering indexes.
- Each truth overwrite closes the previous value's valid-time interval into an append-only history; the current value stays a single upserted row. Re-writing an unchanged value records nothing.

### Upgrade note

Document embeddings are 768-dimensional. Existing document chunks are dropped automatically on first open and must be re-ingested. Graph data is unchanged.

## v0.4.1 — Review Hardening & Performance

### Fixed
- Scoped `rm-obs --id` and `update-obs --id` mutations to the named entity.
- Preserved truths across JSON export/import and rebuilt observation FTS after legacy ID migration.
- Hardened skill repository cloning against option and extended-transport injection.
- Made reset clear topics and vector chunks, exports use `0600` permissions, and duplicate `new --obs` calls idempotent.

### Changed
- Standardized skill installation and vector insertion on immediate transactions.
- Batched recall topic metadata lookups and moved fastembed inference to blocking worker threads.
- Corrected Compact help text to describe duplicate-topic reporting.

## v0.4.0 — SQLite Concurrency & Sandbox Resiliency

### Added
- **Dynamic Busy Timeout**: Read and apply lock timeout dynamically from `ASOBI_BUSY_TIMEOUT` (defaulting to 15000ms).
- **Actionable Open Errors**: Wrapped database directory creation and database building steps with detailed contexts including the resolved file path and workspace setup hints.
- **Journal Mode Override & Fallback**: Support explicit `ASOBI_JOURNAL_MODE` configuration and fall back automatically to `DELETE` journal mode if WAL's shared memory (`-shm` / `-wal`) allocation fails.
- **Database Stats Diagnostics**: `asobi stats` (in both plain-text and JSON outputs) now includes the resolved database file path and active journal mode.
- **Concurrency regression test**: Added a multi-process concurrency integration test suite (`tests/concurrency_test.rs`) verifying execution under bursty lock contention.

### Changed
- **Schema-Version Gate**: Short-circuits connection setup (skipping setup DDLs) if `PRAGMA user_version` matches `SCHEMA_VERSION = 1`, making warm starts completely lock-free.
- **Immediate Setup Lock**: Wrapped cold setup and migrations in `BEGIN IMMEDIATE` and re-check versioning to resolve concurrent initialization race conditions.
- **Immediate Write Transactions**: Configured all graph write operations (`create_entities`, `add_observations`, `create_relations`, `delete_entities`, `delete_observations`, and `delete_relations`) to use `TransactionBehavior::Immediate` to prevent deadlocks under concurrency.

## v0.3.0 — Agent-Centric Performance & Precision


### Added
- **Sequential Observation IDs**: Transitioned the database schema of `asobi_observations.id` from random UUID strings to an `AUTOINCREMENT INTEGER`. Existing databases are automatically migrated in-place upon initialization.
- **Detailed Traversal with IDs (`show --with-ids`)**: The detailed output now includes unique integer `id` values for all observations.
- **Subtree Relation Expansion (`show --expand <type>`)**: Added a repeatable `--expand` flag to `show` (e.g. `--expand part_of`), which recursively traverses and resolves related entities in a single JSON payload.
- **Atomic updates by ID (`update-obs <name> <id> <content> --id`)**: Added support for updating observations by their sequential ID in a single step.
- **ID-Based Deletions (`rm-obs <name> <id> --id`)**: Added support for deleting observations by their sequential ID, removing string-matching ambiguity and avoiding long argument payload overhead.
- **JSON output for stats (`stats --json`)**: Added structured JSON serialization to `asobi stats` for machine readability.
- **Consistent JSON receipts**: Global `--json` flag now outputs structured receipts for `backup`, `restore`, `import`, and `reset` commands.

### Changed
- **$O(1)$ Search Deduplication**: Refactored `search_nodes_with_limit` duplicate resolution to use `HashSet` instead of $O(n)$ `Vec::contains` lookups.
- **Dropped Prefix Deletion**: Replaced the short-lived `rm-obs --prefix` flag with ID-based deletions to prevent concurrency issues and ensure strict matching logic.

## v0.2.2 — Compact hardening

### Fixed

- **Topic frontmatter is strict-YAML safe.** Compacted topic frontmatter (`title`/`type`/`slug` and the new metadata keys) is YAML-quoted, so entity names containing `:` or a leading `#`/`@`/`[` no longer break strict consumers (Obsidian, Dataview). A shared `frontmatter` module now owns the quote-on-write / unquote-on-read contract for `compact`, `ingest`, and `skills` so they can't drift.
- **Re-ingest no longer truncates on a body `---`.** The frontmatter parser matches a whole-line `---` fence instead of the first `\n---` substring, so a thematic break or dash-rule inside a topic body can't cut the document short.

### Changed

- **`compact` is idempotent.** An unchanged entity is left byte-for-byte (its `compacted` timestamp is preserved, not bumped) and is not re-embedded; only entities whose graph state actually changed are rewritten. Stops repeated `compact` runs from churning the vector index.
- **Richer, machine-readable topic output.** Frontmatter now promotes `aliases`, observation/relation counts, each truth as a `truth_<key>` property, outgoing relations as wikilinks, and a `compacted` date; Truths and Relations render as Markdown tables under a `# <name>` heading.
- **Default observation cap raised from 50 to 200.** Still overridable via `ASOBI_OBSERVATION_LIMIT` or `asobi.toml`'s `observation_limit`; truths remain exempt.

## v0.2.1 — Compact fixes

### Fixed

- **`compact` now persists truths.** `sync_graph_to_markdown` only emitted observations and relations, silently dropping every truth — so the compacted Markdown / FTS / vector index lost all current-state facts (`status`, `next`, `title`, `objective`, …). The recall tier was archiving the trail but never the state.

### Changed

- **`compact` syncs durable knowledge only.** Volatile state (`session`, `task`/epic) and self-indexing `skill` entities are no longer written to the recall tier — they were churning the vector index and polluting semantic `query` results, and are already cheaply readable from the graph via `search` / `show`. Knowledge entities (`project`, `concept`, `reference`, `preference`, `standard`) still sync. Use `export` / `backup` for full archival. Previously `skill` entities were re-synced as body-less duplicate topics under a mismatched slug.

## v0.2.0 — Full de-MCP

### Changed (breaking)

- **Flat terse CLI verbs (hard cut, no aliases).** Commands are renamed: `create-entities`→`new`, `add-observations`→`obs`, `create-relations`→`link`, `delete-entities`→`rm`, `delete-observations`→`rm-obs`, `delete-relations`→`unlink`, `read-graph`→`graph`, `search-nodes`→`search`, `open-nodes`→`show`, `add-truth`→`truth`, `delete-truth`→`rm-truth`. Old names no longer resolve.
- **Native `asobi_*` database schema.** Tables renamed from `mcp_*` to `asobi_*`; opening an existing 0.1.x database migrates it in place (FTS/triggers/index are rebuilt). `backup`/`restore` round-trip the new format. **Reinstall the binary (`cargo install asobi`) before opening a v0.1 database with v0.2.**

### Added

- **`search --where key=value`** — filter entities by truths (repeatable, AND-combined); the query term is now optional, so `search --where status=READY_TO_DISPATCH` returns matching entities with no search text. Makes a status board a single O(1) read.
- **`query --json` / `--limit N`** — structured, scriptable semantic-recall output (`documents` feature).
- **`new NAME TYPE --obs "…"`** — seed observations at entity creation (repeatable), collapsing session-save write amplification.
- **Concurrent-write reliability** — the database opens with `journal_mode=WAL`, `synchronous=NORMAL`, and `busy_timeout`, so a lead agent and dispatched sub-agents can write without lock errors.
- **Status-as-truth convention** — task/session status lives in a truth (current state); observations hold transition notes. Documented in `SKILL.md` and `docs/usage.md`.

### Removed

- **MCP server and the `mcp` command.** Asobi is a standalone CLI; the stdio MCP server is gone.

---

## v0.1.0 — First crates.io release

### Changed

- **Published to crates.io as `asobi`** (`cargo install asobi`). The binary is `asobi`, env vars are `ASOBI_*` (`ASOBI_HOME`, `ASOBI_DATABASE_URL`, `ASOBI_OBSERVATION_LIMIT`, …), the workspace dir is `.asobi/`, config is `asobi.toml`, the database is `asobi.db`, and the XDG root is `$XDG_DATA_HOME/asobi/`.
- **Release pipeline publishes to crates.io**: tagging `v*` now runs `cargo publish` (gated on `CARGO_REGISTRY_TOKEN`) alongside the GitHub binary release.
- **`CHANGELOG.md` moved to the repository root.**

---

## Pre-v0.1.0

Condensed history of the project's development prior to its first crates.io release.

- **XDG layout & skill sync** — unified user-level workspace under a single `$XDG_DATA_HOME/asobi/` root; `skills update` / `skills install --all` prune orphaned skills.
- **Truths, observation cap, lazy reads, skills** — structured key-value truths tier (`add-truth` / `delete-truth`); rolling observation cap (`ASOBI_OBSERVATION_LIMIT`, default 50); lazy `read-graph` / `search-nodes`; `skills` subsystem with a persistent clone cache.
- **Unified libSQL storage** — moved vector embeddings from LanceDB into libSQL; all graph + FTS + vector data lives in a single `asobi.db`.
- **Hierarchical normalization** — entity names preserve `:`, `.`, `_`, `-` and original casing; FTS5-friendly segment discovery.
- **Environment isolation** — dropped `.env` autoloading; namespaced all tool variables under `ASOBI_*`.
- **CLI enhancements** — `--version`, `stats`, `export` / `import`, and `reset` subcommands.
- **Workspace path discovery** — walk up to the nearest config like `cargo` / `git`; `ASOBI_HOME` as the highest-priority override.
- **Memory consistency & expansion** — canonical key normalization to dedup entities; 1-hop neighbor expansion in `search-nodes`.
- **Knowledge graph CLI (MCP spec)** — nine Memory-MCP-compatible subcommands; FTS5-powered `search-nodes`; MCP stdio server; optional `documents` feature for ingest / query / compact.
- **Initial setup** — document ingestion with libSQL + LanceDB, FTS5 on topics, and agent infrastructure.
