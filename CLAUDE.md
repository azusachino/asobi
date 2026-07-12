# Asobi

Persistent **knowledge graph CLI** for humans and LLM agents — entities, observations, truths, and relations in a local libSQL database (Turso is an experimental opt-in), with optional semantic recall over ingested Markdown.

## Stack

Rust (edition 2024) on `tokio`; `clap` CLI, `tracing` logs (stderr, `RUST_LOG`). Storage: libSQL by default (graph + FTS5 + vectors) in one `.asobi/` (project-local) or XDG db; Turso is compiled and selectable only behind `--features turso-experimental`. Document tier (`fastembed`, `walkdir`, `text-splitter`) is gated behind `--features documents`. Python 3.14 scripts via `uv`.

## Layout

- `src/main.rs` — CLI dispatch · `src/application.rs` — `AsobiRuntime` composition root · `src/storage/mod.rs` — `Storage` composite over `api::v1` capabilities · `src/storage/libsql/db.rs` — default provider: schema, graph CRUD, FTS5 · `src/storage/turso/` — experimental provider (feature-gated) · `src/model.rs` — graph I/O types
- `src/paths.rs` — workspace resolution (project-local > XDG) · `src/init.rs` — `asobi init` · `src/skills.rs` — skills install/parse
- `src/ingest.rs`, `src/chunk.rs`, `src/embed/`, `src/vector.rs`, `src/recall.rs` — document tier
- `src/compact.rs`, `src/digest.rs` — maintenance · physical backup/restore is a provider capability (`src/storage/<provider>/backup.rs`) · `docs/` — reference · `scripts/` — `uv` utilities

## CLI

Graph: `new` (`--obs` seeds at creation), `obs`, `link`, `rm`, `rm-obs`, `unlink`, `graph`, `search` (`--where key=value` truth filters; query term optional), `show`. Truths: `truth`, `rm-truth`. Plus `skills`, `ingest`, `query` (`--json`/`--limit`), `compact`, `init`, `backup`, `restore`, `stats`, `export` (`--scope <entity>` for a single-epic subgraph bundle, `--rationale` for the decision chain), `import`, `reset`, and `schema [--command NAME]`. Machine-readable payloads retain their command-specific JSON shapes; `asobi schema` is the compatibility promise and discovery surface. See [`docs/response-contract.md`](docs/response-contract.md). Full reference: [`SKILL.md`](SKILL.md), [`docs/usage.md`](docs/usage.md).

## Make

`make check` (fmt + lint + test + test-scripts + check-documents) is the CI baseline and must pass before commit (quality-gate hook). It runs the **default libSQL build only**. The experimental Turso backend is opt-in: `make check-turso` builds/tests/verifies it behind `--features turso-experimental`. Also `make build` / `build-documents` / `test-documents` / `bench`.

## Conventions

Standard Rust naming; `anyhow` at boundaries, `thiserror` in core. Tests single-threaded (shared `ASOBI_DATABASE_URL`), embedded in modules. Formatters: `rustfmt`, `prettier` (JSON/YAML), `ruff`. No clippy warnings, no skipped formatters.

DB schema is `asobi_*` (migrated in place from the legacy `mcp_*` on open). Each provider owns its own state-file name (libSQL: `asobi.db`, Turso: `asobi.turso.db`) and its own concurrency model (database opening and writes have bounded retry handling for transient lock contention); the application layer never names a provider, driver, SQL, or state file — see `docs/specs/2026-07-11-storage-composition-adr.md` and the `scripts/verify_storage_boundary.py` gate. **Status-as-truth**: an entity's `status` lives in a truth (current state), observations hold transition notes — so a board is a single `search --where status=…`.

## Bash hygiene (HARD RULE)

Run **one plain command per Bash call.** No ceremony.

- **Never `cd`** — the working directory is already the repo root and persists between calls.
- **Never** chain `>file 2>&1; echo $status; tail …` or similar `&&`/`;` pipelines just to inspect output. That pattern triggers a permission prompt every time. Run the command bare (e.g. `make check`); the harness surfaces the exit code, and the full output is tee'd to a log path you can open with the **Read tool**.
- **Never use `echo`** for section headers/labels, and **never** `cat`/`head`/`tail`/`sed`/`awk` to read files — use the **Read** tool. Use `rg`/`fd` for search.
