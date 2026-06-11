# Rosemary

Rosemary is a persistent, project-local knowledge graph CLI for AI agents. Agents use it to keep memory, track session state, and share context across conversations — stored in a local libSQL/SQLite file, no server required.

## Features

- **Knowledge graph** — entities, append-only (capped) observations, and directed relations.
- **Truths** — durable `key→value` facts per entity for current state (`status`, `version`).
- **Fast search** — `search-nodes` over FTS5 (porter stemming + BM25) with a substring fallback.
- **Lazy reads** — `read-graph`/`search-nodes` return truths + counts; `open-nodes` returns the full body. Cheap to load, cheap on tokens.
- **Skills** — install reusable agent instructions from a git repo or local path.
- **MCP server** — `rosemary mcp` serves the graph over stdio to MCP-aware clients.
- **Document tier** (optional, `--features documents`) — `ingest` + semantic `query` over Markdown.

## Installation

From source (Rust 1.85+, Edition 2024):

```bash
cargo install --git https://github.com/azusachino/rosemary
```

Build locally with `make build` (graph/MCP CLI) or `make build-documents` (adds `ingest`/`query`/`compact`).

## Quick Start

```bash
rosemary init                  # one-time setup (XDG); use --local for a project-scoped graph

# Store and recall context (names are hierarchical, e.g. ame:mobile-support:task-1)
rosemary add-observations "my-project" "Decided to use WAL mode for concurrency"
rosemary add-truth "my-project" "status" "in-progress"
rosemary search-nodes "WAL"
rosemary open-nodes "my-project"
```

## Common Commands

- `rosemary read-graph` / `search-nodes <q>` / `open-nodes <name>...` — read the graph.
- `rosemary add-truth <name> <key> <value>` / `delete-truth <name> <key>` — manage truths.
- `rosemary skills install <src> --all` / `skills` / `skills show <name>` — manage skills.
- `rosemary mcp` — run as an MCP stdio server.
- `rosemary stats` / `export -o graph.json` / `import graph.json` / `reset` — inspect & manage.

## Development

- **Task runner**: `make` (Nix-wrapped). Run `make check` for fmt + lint + tests.
- See [`docs/usage.md`](docs/usage.md) for the full CLI reference and [`docs/architecture.md`](docs/architecture.md) for design.
