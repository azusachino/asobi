# Changelog

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
