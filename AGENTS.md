# Rosemary

## Project Overview

Rosemary is a persistent **knowledge graph CLI** for humans and LLM agents. It maintains entities, observations, and relations in a local libSQL file, with optional semantic recall over ingested Markdown documents.

## Tech Stack

- **Language**: Rust (edition 2024); Python 3.14 for ancillary scripts.
- **Storage**:
  - libSQL (SQLite-compatible) for the graph tier — entities, observations, relations, FTS5 indexes.
  - LanceDB + `fastembed` for the document tier — chunked, embedded Markdown for semantic recall.
- **Async runtime**: `tokio`.
- **CLI**: `clap` (derive).
- **Other**: `serde`/`serde_json`, `anyhow`, `chrono`, `uuid`, `directories`; optional document-tier crates include `walkdir`, `text-splitter`, `fastembed`, and LanceDB/Arrow.

## Repository Layout

- `src/main.rs` — CLI entry point and subcommand dispatch.
- `src/lib.rs` — module roots.
- `src/db.rs` — libSQL schema, graph CRUD, FTS5 search.
- `src/mcp.rs` — MCP 2024-11-05 stdio server and shared JSON types.
- `src/paths.rs` — workspace path resolution (project-local > XDG).
- `src/init.rs` — `rosemary init` workspace bootstrap.
- `src/ingest.rs`, `src/chunk.rs`, `src/embed/`, `src/vector.rs`, `src/recall.rs` — document tier pipeline.
- `src/compact.rs`, `src/digest.rs` — maintenance and session digest helpers.
- `docs/` — architecture, usage guide, design plans, changelog.
- `scripts/` — Python utilities (managed via `uv`).

Runtime data lives under `.rosemary/` (project-local) or the XDG data dir.

## CLI Surface

All nine `@modelcontextprotocol/server-memory` graph methods are implemented as both CLI subcommands and MCP tools:

- `create-entities`, `add-observations`, `create-relations`
- `delete-entities`, `delete-observations`, `delete-relations`
- `read-graph`, `search-nodes`, `open-nodes`

Plus document/maintenance/workflow commands: `ingest`, `query`, `compact`, `init`, `mcp`.

See [`SKILL.md`](SKILL.md) and [`docs/usage.md`](docs/usage.md) for the full reference.

## Build, Run & Test

Day-to-day work goes through `make`:

- `make fmt` — format Rust, JSON/YAML, and Python.
- `make lint` — clippy (`-D warnings`) plus ruff.
- `make test` — cargo test (single-threaded; tests share `DATABASE_URL`).
- `make test-scripts` — `uv`-managed CLI integration checks.
- `make check` — `fmt` + `lint` + `test` + `test-scripts`. CI baseline.
- `make build` — debug build of the CLI.
- `make build-documents` / `make test-documents` — enable the optional document tier.
- `make bench` — graph-tier benchmark harness.

## Coding Conventions

- Standard Rust naming (snake_case / PascalCase).
- `anyhow` at application boundaries.
- Table-driven tests where they fit; integration tests embedded in modules.
- Formatters: `rustfmt`, `prettier` (JSON/YAML), `ruff`.
- Python via `uv` (Python 3.14).

## Quality Standards

- `make check` must pass before commit (enforced by the local quality-gate hook).
- No `clippy` warnings.
- No skipped formatters.
