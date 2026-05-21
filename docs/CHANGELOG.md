# Changelog

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

## v0.1.0 — 2026-04-08

Initial project setup. Basic document ingestion with libSQL + LanceDB, FTS5 on topics. Agent infrastructure (`AGENTS.md`, `.claude/rules/`, `GEMINI.md`, `flake.nix`, `Makefile`).
