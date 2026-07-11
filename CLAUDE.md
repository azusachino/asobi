# Asobi

Persistent **knowledge graph CLI** for humans and LLM agents — entities, observations, truths, and relations in a local libSQL file, with optional semantic recall over ingested Markdown.

## Stack

Rust (edition 2024) on `tokio`; `clap` CLI, `tracing` logs (stderr, `RUST_LOG`). Storage: libSQL (graph + FTS5 + vectors) in one `.asobi/` (project-local) or XDG db. Document tier (`fastembed`, `walkdir`, `text-splitter`) is gated behind `--features documents`. Python 3.14 scripts via `uv`.

## Layout

- `src/main.rs` — CLI dispatch · `src/db.rs` — schema, graph CRUD, FTS5 · `src/model.rs` — graph I/O types
- `src/paths.rs` — workspace resolution (project-local > XDG) · `src/init.rs` — `asobi init` · `src/skills.rs` — skills install/parse
- `src/ingest.rs`, `src/chunk.rs`, `src/embed/`, `src/vector.rs`, `src/recall.rs` — document tier
- `src/compact.rs`, `src/digest.rs`, `src/backup.rs` — maintenance · `docs/` — reference · `scripts/` — `uv` utilities

## CLI

Graph: `new` (`--obs` seeds at creation), `obs`, `link`, `rm`, `rm-obs`, `unlink`, `graph`, `search` (`--where key=value` truth filters; query term optional), `show`. Truths: `truth`, `rm-truth`. Plus `skills`, `ingest`, `query` (`--json`/`--limit`), `compact`, `init`, `backup`, `restore`, `stats`, `export` (`--scope <entity>` for a single-epic subgraph bundle, `--rationale` for the decision chain), `import`, `reset`. Full reference: [`SKILL.md`](SKILL.md), [`docs/usage.md`](docs/usage.md).

## Make

`make check` (fmt + lint + test + test-scripts) is the CI baseline and must pass before commit (quality-gate hook). Also `make build` / `build-documents` / `test-documents` / `bench`.

## Conventions

Standard Rust naming; `anyhow` at boundaries, `thiserror` in core. Tests single-threaded (shared `ASOBI_DATABASE_URL`), embedded in modules. Formatters: `rustfmt`, `prettier` (JSON/YAML), `ruff`. No clippy warnings, no skipped formatters.

DB schema is `asobi_*` (migrated in place from the legacy `mcp_*` on open). The connection opens with WAL + `synchronous=NORMAL` + `busy_timeout` so a lead and dispatched sub-agents can write concurrently. **Status-as-truth**: an entity's `status` lives in a truth (current state), observations hold transition notes — so a board is a single `search --where status=…`.

## Bash hygiene (HARD RULE)

Run **one plain command per Bash call.** No ceremony.

- **Never `cd`** — the working directory is already the repo root and persists between calls.
- **Never** chain `>file 2>&1; echo $status; tail …` or similar `&&`/`;` pipelines just to inspect output. That pattern triggers a permission prompt every time. Run the command bare (e.g. `make check`); the harness surfaces the exit code, and the full output is tee'd to a log path you can open with the **Read tool**.
- **Never use `echo`** for section headers/labels, and **never** `cat`/`head`/`tail`/`sed`/`awk` to read files — use the **Read** tool. Use `rg`/`fd` for search.
