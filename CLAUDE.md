# Asobi

Persistent **knowledge graph CLI** for humans and LLM agents — entities, observations, truths, and relations in a local libSQL file, with optional semantic recall over ingested Markdown.

## Stack

Rust (edition 2024) on `tokio`; `clap` CLI, `tracing` logs (stderr, `RUST_LOG`). Storage: libSQL (graph + FTS5 + vectors) in one `.asobi/` (project-local) or XDG db. Document tier (`fastembed`, `walkdir`, `text-splitter`) is gated behind `--features documents`. Python 3.14 scripts via `uv`.

## Layout

- `src/main.rs` — CLI dispatch · `src/db.rs` — schema, graph CRUD, FTS5 · `src/mcp.rs` — MCP stdio server
- `src/paths.rs` — workspace resolution (project-local > XDG) · `src/init.rs` — `asobi init` · `src/skills.rs` — skills install/parse
- `src/ingest.rs`, `src/chunk.rs`, `src/embed/`, `src/vector.rs`, `src/recall.rs` — document tier
- `src/compact.rs`, `src/digest.rs`, `src/backup.rs` — maintenance · `docs/` — reference · `scripts/` — `uv` utilities

## CLI

Graph (CLI + MCP): `create-entities`, `add-observations`, `create-relations`, `delete-*`, `read-graph`, `search-nodes`, `open-nodes`. Truths: `add-truth`, `delete-truth`. Plus `skills`, `ingest`, `query`, `compact`, `init`, `mcp`, `backup`, `restore`. Full reference: [`SKILL.md`](SKILL.md), [`docs/usage.md`](docs/usage.md).

## Make

`make check` (fmt + lint + test + test-scripts) is the CI baseline and must pass before commit (quality-gate hook). Also `make build` / `build-documents` / `test-documents` / `bench`.

## Conventions

Standard Rust naming; `anyhow` at boundaries, `thiserror` in core. Tests single-threaded (shared `ASOBI_DATABASE_URL`), embedded in modules. Formatters: `rustfmt`, `prettier` (JSON/YAML), `ruff`. No clippy warnings, no skipped formatters.
