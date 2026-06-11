# Changelog

## v0.7.0 — Rename: rosemary → miku

### Changed

- **Project renamed `rosemary` → `miku`** and first published to crates.io (`cargo install miku`). This is a breaking change: the binary is now `miku`, env vars are `MIKU_*` (`MIKU_HOME`, `MIKU_DATABASE_URL`, `MIKU_OBSERVATION_LIMIT`, …), the workspace dir is `.miku/`, config is `miku.toml`, the database is `miku.db`, and the XDG root is `$XDG_DATA_HOME/miku/`. Existing `.rosemary/` workspaces do not migrate automatically — re-init under the new layout.
- **Release pipeline publishes to crates.io**: tagging `v*` now runs `cargo publish` (gated on `CARGO_REGISTRY_TOKEN`) alongside the GitHub binary release.
- **`CHANGELOG.md` moved to the repository root.**

---

## v0.6.1 — XDG Layout & Skill Sync

### Changed

- **Unified XDG workspace**: The user-level layout now lives under a single `$XDG_DATA_HOME/miku/` root holding the same `{data,config,topics,caches}` subtree as a project-local `.miku/`, instead of splitting across `~/.local/share` and `~/.config`. `XDG_DATA_HOME` is honored on **every** platform — dropped the `directories` crate, which previously ignored XDG vars on macOS and stored data under `~/Library/Application Support` (and dumped skill clones into a shared `Application Support/caches`).

### Fixed

- **Skill sync prunes orphans**: `skills update` and `skills install --all` now sync — skills previously installed from a source but deleted or renamed upstream are pruned, so the graph mirrors the source. `--select` and the interactive picker remain purely additive.

---

## v0.6.0 — Truths, Observation Cap, Lazy Reads, and Skills Subsystem

### New features

- **Truths Tier**: Added support for structured, non-text-searchable key-value pairs (truths) attached to entities, providing a durable structured knowledge tier. Supported via CLI (`add-truth`, `delete-truth`) and MCP tools (`add_truth`, `delete_truth`).
- **Observation Cap**: Added a rolling history cap for observations (`MikuConfig.observation_limit` / `MIKU_OBSERVATION_LIMIT`, default 50). Inserting observations beyond the cap evicts oldest observations in the same transaction.
- **Lazy-Read Contract**: Optimizes token overhead for agents. `read-graph` and `search-nodes` are now lazy (returning only `truths` and `observation_count` with empty `observations`), while `open-nodes` remains eager (populating all observations).
- **Skills Subsystem**: Reusable agent instructions and technical workflows. Added CLI command group `skills` (`install`, `update`, `remove`, `list`) supporting installation from git clones, frontmatter metadata parsing, and cascading body storage.
- **Persistent Skills Cache**: Repositories are now cloned to `.miku/caches/{slug}` instead of a transient directory, allowing offline browsing and faster incremental updates via `git fetch` and `git reset --hard`.
- **Flexible Frontmatter Parsing**: Relaxes frontmatter requirements by falling back to the file-stem or parent directory name for the skill name, and defaulting the description to an empty string.
- **Skill Show Command**: Added `miku skills show <name>` to output raw unescaped markdown body contents of skills for humans to read without JSON escaping.
- **Line Ending Normalization**: Automatically normalizes CRLF (`\r\n`) to LF (`\n`) for all imported skill bodies and frontmatters.
- **Document Feature Skill Embedding**: Enabled embedding skill bodies into the document-tier vector store on install/update under the `--features documents` build gate.

---

## v0.5.0 — Unified libSQL Storage

### Changed

- **Unified libSQL Storage**: Moved vector embeddings from LanceDB to libSQL using its native vector search support.
- **Dependency Reduction**: Removed `lancedb` and `arrow` dependencies, significantly reducing build times and binary size overhead.
- **Single Source of Truth**: All data (graph, FTS, and vectors) now resides in a single `miku.db` file.
- **Simplified Initialization**: Graph and document tiers now share the same libSQL connection, eliminating separate database initialization paths.

---

## v0.4.2 — Hierarchical Normalization

### New features

- **Hierarchical Key Normalization**: Re-designed the entity name normalization to support hierarchical naming schemes.
    - **Symbol Preservation**: Characters like `:`, `.`, `_`, and `-` are now preserved in entity names.
    - **Case Preservation**: Entity names now maintain their original casing (e.g., `UserPreferences` stays `UserPreferences`), avoiding the memory fragmentation caused by aggressive `kebab-case` folding.
    - **FTS-Friendly**: The FTS5 index naturally supports discovery of key segments. Searching for `ame` or `mobile` will correctly find `ame:mobile-support:task-1`.

### Fixes

- **Case-Fold Duplicate Bug**: Fixed a bug where `UserPreferences` and `userpreferences` would be treated as the same entity during normalization but could conflict in the database.

---

## v0.4.1 — Environment Isolation & Safety

### Fixes

- **Environment Variable Isolation**: Removed automatic `.env` loading from the current directory. Global CLI tools should not "leach" from local project environments, which frequently caused `DATABASE_URL` collisions in developer projects.
- **Namespaced Environment Variables**: Prefixed all tool-specific variables with `MIKU_` to prevent namespace pollution.
    - `DATABASE_URL` → `MIKU_DATABASE_URL`
    - `LANCEDB_PATH` → `MIKU_LANCEDB_PATH`
    - `FASTEMBED_CACHE_DIR` → `MIKU_FASTEMBED_CACHE_DIR`
- **API Key Safety**: Added support for `MIKU_ANTHROPIC_API_KEY` to allow isolating memory-assistant keys from project-level keys.

## v0.4.0 — CLI Enhancements

### New features

- **`--version` support**: Added native clap support for displaying the current CLI version via `miku --version`.
- **`stats` subcommand**: New `miku stats` command to quickly inspect the knowledge graph size (entities, relations, observations).
- **`export` / `import` subcommands**: New `miku export -o graph.json` and `miku import graph.json` commands to backup, share, and restore the knowledge graph.
- **`reset` subcommand**: New `miku reset` command to clear the knowledge graph (requires interactive `[y/N]` confirmation, or `--force`).

## v0.3.1 — Workspace Path Discovery Fix

### Fixes

- **`miku compact` and other commands now honor the configured workspace location** when invoked from a subdirectory. Previously, `MikuPaths::resolve()` only checked cwd for `miku.toml` / `.miku/`, so running a command from a subdir silently fell through to XDG (or created a new `.miku/` in the wrong place). It now walks up from cwd to find the nearest config, matching how `cargo` and `git` discover their roots.
- **Relative paths in `miku.toml` are now anchored to the config file's directory**, not cwd. The seeded `miku.toml` already advertised this behavior in its comment ("Paths are resolved relative to this file") — the implementation now matches.
- **`MIKU_HOME` is now the highest-priority override**, bypassing project-local discovery entirely.
- **`fastembed` model cache no longer leaks into cwd.** The provider's default `cache_dir` was `./.fastembed_cache`, which polluted any project where `miku` was invoked. It now defaults to `<data_dir>/fastembed_cache` (or `FASTEMBED_CACHE_DIR` if set), keeping all workspace state inside the configured location.

## v0.3.0 — Memory Consistency & Expansion (feat/memory-improvements)

### Summary

This release introduces canonical key normalization for the MCP memory graph, ensuring entities and relations share a consistent `kebab-case` namespace to avoid memory fragmentation. Additionally, graph search has been enhanced to automatically perform 1-hop neighbor expansion, providing agents with richer context during discovery.

### New features

- **Canonical Key Normalization**: All incoming `name` and `entity_name` fields are strictly normalized to lowercase kebab-case before ingestion. This deduplicates entities created under different casing/spacing variations.
- **1-Hop Neighbor Expansion**: `search-nodes` now automatically fetches and includes the 1-hop relations (edges) for all matched nodes, giving agents surrounding context.
- **Verbose Tool Responses**: The MCP tools for `create_entities` and `add_observations` now return the serialized, updated state of the graph immediately, removing the need for an extra `read_graph` validation call.

### Performance

- **Broad Search Overhaul**: The normalization deduplication inherently optimizes SQLite FTS and pattern matching. Broad search queries return ~48% faster (from 283ms down to 146ms for 10,000 matches).

### Fixes

- Fixed potential SQL constraint violations during entity generation by enforcing strict ASCII alphanumeric normalization.
- Refactored legacy `tests/graph_edge_cases.rs` to enforce canonical configurations.

---

## v0.2.0 — pre-release (feat/mcp-knowledge-graph)

### Summary

Miku pivots from an async Rust learning project to a production-grade knowledge graph CLI for LLM agents. The graph tier is now the primary interface.

### Breaking changes

- CLI subcommand names changed to match the Memory MCP spec. If you have scripts using the old names, update them:

| Old                           | New                   |
| ----------------------------- | --------------------- |
| `add-entity`                  | `create-entities`     |
| `add-obs` / `add-observation` | `add-observations`    |
| `relate`                      | `create-relations`    |
| `list`                        | `read-graph`          |
| `delete-entity`               | `delete-entities`     |
| `delete-observation`          | `delete-observations` |

### New features

#### Knowledge graph CLI (Memory MCP spec compatible)

Nine subcommands aligned with `@modelcontextprotocol/server-memory`:

```
create-entities   add-observations  create-relations
delete-entities   delete-observations  delete-relations
read-graph        search-nodes      open-nodes
```

All graph operations complete in <10ms. No model startup cost.

#### FTS5-powered `search-nodes`

`search-nodes` now uses SQLite FTS5 (Full-Text Search 5) with porter stemming and BM25 ranking:

- `search-nodes "run"` matches `"running"`, `"runner"`, `"ran"`
- Multi-word queries rank entities with both words higher
- FTS5 operators: `AND`, `OR`, `NOT`, prefix `*`
- Falls back to substring LIKE on entity name/type (catches exact-name lookups and entities with no observations)
- Invalid FTS5 syntax degrades gracefully to LIKE
- Defaults to top 100 matched nodes; use `--limit` or MCP `limit` for larger exports
- Batch-loads matched entities/observations and indexes `mcp_observations(entity_name)` to avoid N+1 observation reads

#### MCP stdio server

`miku mcp` is a fully compliant MCP 2024-11-05 server:

- `initialize` handshake with capabilities negotiation
- `tools/list` with input schemas for all 9 tools
- `tools/call` dispatch with `content[{type, text}]` response format
- Notifications (`notifications/initialized`) correctly ignored

Register with Claude Code: `claude mcp add miku -- miku mcp`

#### Lazy vector initialization

Graph commands (`create-entities`, `read-graph`, `search-nodes`, etc.) no longer initialize LanceDB or the fastembed model. Only `ingest`, `query`, and `compact` pay the model load cost.

#### Optional document-tier feature

LanceDB, fastembed, Arrow, token splitting, and directory ingest dependencies are now behind Cargo feature `documents`. Default builds include the graph/MCP CLI only; build with `--features documents` or `make build-documents` to enable `ingest`, `query`, and `compact`.

#### Scripted CLI integration checks

`scripts/verify_cli.py` now runs graph-only CLI integration checks via `uv`, covering entity creation, observations, relations, FTS fallback, deletion, and JSON output parsing.

#### Project-local storage

Miku auto-detects project scope in priority order:

1. `miku.toml` in current directory
2. `.miku/` directory in current directory
3. XDG paths (`~/.local/share/miku/`)

Agents in different repos keep separate graphs automatically.

### Fixes

- Removed dead code (`upsert_entity`, `insert_relation`, `get_related`) referencing non-existent tables
- Fixed `SKILL.md` command names (was referencing pre-refactor API)
- Fixed `verify_cli.py` command names
- Fixed clippy lints in `compact.rs` (`push_str("\n")` → `push('\n')`) and `paths.rs` (collapsible if)
- Removed the stale async-learning track, gRPC proto/build script, and related dependencies before public release.

### Documentation

- `README.md` — rewritten with proper project overview and quick start
- `docs/architecture.md` — design decisions, tier diagram, FTS5 rationale, performance headroom
- `docs/usage.md` — human workflows and agent integration guide
- `SKILL.md` — rewritten with correct command signatures, output formats, session protocol

### Known limitations / next

- `search-nodes` uses `Vec::contains` for dedup — O(n) per lookup, fine for <1k results
- No index on `mcp_observations.entity_name` — sequential scans for observation loads
- `compact` always re-embeds, even for unchanged entities
- WAL journal mode not yet enabled — concurrent agent writers serialize
- `mcp_search_nodes` and `mcp_open_nodes` have N+1 observation query pattern

See [`docs/architecture.md`](architecture.md#performance-headroom) for implementation plans.

---

## v0.9.0 — 2026-05-21

- Public release preparation:
    - Metadata and licensing (MIT).
    - GitHub Actions CI/CD workflows (`mise`-managed).
    - Cleaned up documentation (`README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`).
    - Removed Nix infrastructure.
    - Standardized project toolchain via `mise`.
---

## v0.1.0 — 2026-04-08

Initial project setup. Basic document ingestion with libSQL + LanceDB, FTS5 on topics. Agent infrastructure (`AGENTS.md`, `.claude/rules/`, `GEMINI.md`, `Makefile`).
