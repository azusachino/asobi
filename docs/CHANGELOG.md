# Changelog

## v0.3.1 — Workspace Path Discovery Fix

### Fixes

- **`rosemary compact` and other commands now honor the configured workspace location** when invoked from a subdirectory. Previously, `RosemaryPaths::resolve()` only checked cwd for `rosemary.toml` / `.rosemary/`, so running a command from a subdir silently fell through to XDG (or created a new `.rosemary/` in the wrong place). It now walks up from cwd to find the nearest config, matching how `cargo` and `git` discover their roots.
- **Relative paths in `rosemary.toml` are now anchored to the config file's directory**, not cwd. The seeded `rosemary.toml` already advertised this behavior in its comment ("Paths are resolved relative to this file") — the implementation now matches.
- **`ROSEMARY_HOME` is now the highest-priority override**, bypassing project-local discovery entirely.
- **`fastembed` model cache no longer leaks into cwd.** The provider's default `cache_dir` was `./.fastembed_cache`, which polluted any project where `rosemary` was invoked. It now defaults to `<data_dir>/fastembed_cache` (or `FASTEMBED_CACHE_DIR` if set), keeping all workspace state inside the configured location.

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

Rosemary pivots from an async Rust learning project to a production-grade knowledge graph CLI for LLM agents. The graph tier is now the primary interface.

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

`rosemary mcp` is a fully compliant MCP 2024-11-05 server:

- `initialize` handshake with capabilities negotiation
- `tools/list` with input schemas for all 9 tools
- `tools/call` dispatch with `content[{type, text}]` response format
- Notifications (`notifications/initialized`) correctly ignored

Register with Claude Code: `claude mcp add rosemary -- rosemary mcp`

#### Lazy vector initialization

Graph commands (`create-entities`, `read-graph`, `search-nodes`, etc.) no longer initialize LanceDB or the fastembed model. Only `ingest`, `query`, and `compact` pay the model load cost.

#### Optional document-tier feature

LanceDB, fastembed, Arrow, token splitting, and directory ingest dependencies are now behind Cargo feature `documents`. Default builds include the graph/MCP CLI only; build with `--features documents` or `make build-documents` to enable `ingest`, `query`, and `compact`.

#### Scripted CLI integration checks

`scripts/verify_cli.py` now runs graph-only CLI integration checks via `uv`, covering entity creation, observations, relations, FTS fallback, deletion, and JSON output parsing.

#### Project-local storage

Rosemary auto-detects project scope in priority order:

1. `rosemary.toml` in current directory
2. `.rosemary/` directory in current directory
3. XDG paths (`~/.local/share/rosemary/`)

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
