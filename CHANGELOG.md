# Changelog

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
