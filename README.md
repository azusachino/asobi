# Asobi

Asobi is a persistent, project-local knowledge graph CLI for AI agents. Agents use it to keep memory, track session state, and share context across conversations — stored in a local libSQL/SQLite file, no server required.

## Features

- **Knowledge graph** — entities, append-only (capped) observations, and directed relations.
- **Truths** — durable `key→value` facts per entity for current state (`status`, `version`).
- **Fast search** — `search-nodes` over FTS5 (porter stemming + BM25) with a substring fallback.
- **Lazy reads** — `read-graph`/`search-nodes` return truths + counts; `open-nodes` returns the full body. Cheap to load, cheap on tokens.
- **Skills** — install reusable agent instructions from a git repo or local path.
- **Document tier** (optional, `--features documents`) — `ingest` + semantic `query` over Markdown.

## Installation

### From crates.io (recommended)

```bash
cargo install asobi
# with the optional document tier (semantic ingest + query):
cargo install asobi --features documents
```

### Prebuilt binary (cargo-binstall)

No compile — [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) pulls the binary from the GitHub release:

```bash
cargo binstall asobi
```

### From source

```bash
cargo install --git https://github.com/azusachino/asobi
```

Or build locally with `make build` (graph CLI) or `make build-documents` (adds `ingest`/`query`/`compact`). Requires Rust 1.85+, Edition 2024.

## Quick Start

```bash
asobi init                  # one-time setup (XDG); use --local for a project-scoped graph

# Store and recall context (names are hierarchical, e.g. ame:mobile-support:task-1)
asobi add-observations "my-project" "Decided to use WAL mode for concurrency"
asobi add-truth "my-project" "status" "in-progress"
asobi search-nodes "WAL"
asobi open-nodes "my-project"
```

## Common Commands

- `asobi read-graph` / `search-nodes <q>` / `open-nodes <name>...` — read the graph.
- `asobi add-truth <name> <key> <value>` / `delete-truth <name> <key>` — manage truths.
- `asobi skills install <src> --all` / `update` / `skills` / `skills show <name>` — manage skills (`--all` and `update` sync, pruning skills dropped upstream; `--select` is additive).
- `asobi stats` / `export -o graph.json` / `import graph.json` / `reset` — inspect & manage.

## Development

- **Task runner**: `make` (Nix-wrapped). Run `make check` for fmt + lint + tests.
- See [`docs/usage.md`](docs/usage.md) for the full CLI reference and [`docs/architecture.md`](docs/architecture.md) for design.
