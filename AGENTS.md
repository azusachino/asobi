# Rosemary

## Project Overview

Rosemary is a persistent **knowledge graph CLI** for humans and LLM agents. It maintains entities, observations, and relations in a local libSQL file, with optional semantic recall over ingested Markdown documents.

The repo also doubles as a sandbox for async Rust patterns; standalone samples live under `examples/`.

## Tech Stack

- **Language**: Rust (edition 2024); Python 3.14 for ancillary scripts.
- **Storage**:
  - libSQL (SQLite-compatible) for the graph tier — entities, observations, relations, FTS5 indexes.
  - LanceDB + `fastembed` for the document tier — chunked, embedded Markdown for semantic recall.
- **Async runtime**: `tokio`.
- **CLI**: `clap` (derive).
- **Other**: `serde`/`serde_json`, `anyhow`/`thiserror`, `chrono`, `uuid`, `walkdir`, `directories`, `text-splitter`.

## Repository Layout

- `src/main.rs` — CLI entry point and subcommand dispatch.
- `src/lib.rs` — module roots.
- `src/db.rs` — libSQL schema, graph CRUD, FTS5 search.
- `src/mcp.rs` — MCP 2024-11-05 stdio server and shared JSON types.
- `src/paths.rs` — workspace path resolution (project-local > XDG).
- `src/init.rs` — `rosemary init` workspace bootstrap.
- `src/ingest.rs`, `src/chunk.rs`, `src/embed/`, `src/vector.rs`, `src/recall.rs` — document tier pipeline.
- `src/compact.rs`, `src/digest.rs` — maintenance and session digest helpers.
- `examples/` — standalone async Rust samples.
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

- `make fmt` — format Rust, TOML, Markdown, Python.
- `make lint` — clippy (`-D warnings`) plus ruff/pymarkdown.
- `make test` — cargo test (single-threaded; tests share `DATABASE_URL`).
- `make check` — `fmt` + `lint` + `test`. CI baseline.
- `make build` — debug build of the CLI.
- `make run-examples EXAMPLE=name` — run an async sample.

With Nix installed, every target runs inside `nix develop` automatically.

## Coding Conventions

- Standard Rust naming (snake_case / PascalCase).
- `anyhow` at application boundaries; `thiserror` for library-style errors.
- Table-driven tests where they fit; integration tests embedded in modules.
- Formatters: `rustfmt`, `taplo` (TOML), `prettier` (MD/JSON/YAML).
- Python via `uv` (Python 3.14).

## Quality Standards

- `make check` must pass before commit (enforced by the local quality-gate hook).
- No `clippy` warnings.
- No skipped formatters.
