# Asobi

Persistent knowledge-graph CLI for humans and AI agents. Asobi 0.6 stores entities, observations, truths, relations, skills, and task state in a local SQLite database.

## Stack and layout

Rust 2024, Clap, tracing, rusqlite with bundled SQLite/FTS5, and Python scripts run through `uv`.

- `src/main.rs` — thin process entry point
- `src/cli/` — command parsing, routing, output, skills, and runtime setup
- `src/api/v2.rs` — backend-neutral synchronous capability traits
- `src/storage/sqlite.rs` — schema, FTS5, graph CRUD, transactions, backup/restore
- `src/tasks.rs` — durable task planning, dispatch, sync, and close workflows
- `src/compact.rs` — graph-to-Markdown topic projection
- `tests/` — contract, CLI, edge-case, and multi-process verification
- `benches/` — graph, SQLite, task, allocation, and SQL-plan benchmarks

## CLI surface

Graph: `new`, `obs`, `link`, `rm`, `rm-obs`, `update-obs`, `unlink`, `graph`, `search`, and `show`. Truths: `truth`, `rm-truth`, and `history`. Maintenance: `compact`, `init`, `stats`, `schema`, `export`, `import`, `reset`, `backup`, and `restore`. Agent workflows: `skills` and `tasks` with their nested subcommands.

`asobi schema` is the machine-readable response contract. `SKILL.md` and `docs/usage.md` are the user-facing command references.

## Quality gate

Run `make check`. It covers formatting, Clippy, all Rust tests, the CLI verifier, the daily-practice use-case script, and the storage-boundary check. Benchmark compilation is `cargo bench --no-run`; benchmark execution is available through the `make bench-*` targets.

## Conventions

Use synchronous storage operations and immediate SQLite transactions. Keep provider details inside `src/storage/`; commands depend on `api::v2` traits. Status is a truth, while observations record the transition trail. Keep tests isolated with temporary `ASOBI_DATABASE_URL` paths and run them serially when they modify process-wide environment variables.
