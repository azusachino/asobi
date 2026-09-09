# Asobi

Persistent knowledge-graph CLI for humans and AI agents. Asobi 0.7 stores entities, observations, truths, relations, and task state in a local SQLite database. Skills are deliberately **not** in that list: 0.7 moved them onto the filesystem, because the disk copy under `.agents/skills/` is the one agents actually read.

## Stack and layout

Rust 2024, Clap, tracing, rusqlite with bundled SQLite/FTS5, and Python scripts run through `uv`. Tool versions are pinned in `.mise.toml`; `make init` provisions them, and CI resolves the same file.

- `src/main.rs` — thin process entry point
- `src/cli/` — command parsing, routing, output, skills, and runtime setup
- `src/api/v2.rs` — backend-neutral synchronous capability traits
- `src/storage/sqlite.rs` — schema, FTS5, graph CRUD, transactions, backup/restore
- `src/tasks.rs` — durable task planning, dispatch, sync, and close workflows
- `src/skills.rs` / `src/skills_config.rs` — skill parsing and materialization to disk, and the declarative `[skills]` block that `skills sync` reconciles
- `src/frontmatter.rs` — the shared YAML-frontmatter subset. Deliberately not a real YAML parser; read its module doc before widening it
- `src/compact.rs` — graph-to-Markdown topic projection
- `tests/` — contract, CLI, edge-case, and multi-process verification
- `benches/` — graph, SQLite, task, allocation, and SQL-plan benchmarks

## CLI surface

Graph: `new`, `obs`, `link`, `rm`, `rm-obs`, `update-obs`, `unlink`, `graph`, `search`, and `show`. Truths: `truth` and `rm-truth` — a truth is the current value only, since 0.7 removed `history` along with the archive-on-overwrite path. Maintenance: `compact`, `purge`, `init`, `stats`, `capabilities`, `schema`, `completions`, `export`, `import`, `reset`, `backup`, and `restore`. Agent workflows: `skills` and `tasks` with their nested subcommands.

`asobi schema` is the machine-readable response contract, and `docs/usage.md` is the user-facing command reference — the single one. This repository documents what the CLI _is_ and ships no `SKILL.md`; agent workflow guidance for Asobi lives in the [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md). Install that skill with:

```bash
asobi skills install https://github.com/azusachino/harus-skills.git --select asobi
```

Keep the split when adding documentation: a change to a command's behaviour belongs in `docs/usage.md`, and advice about when to reach for it does not belong in this repository at all.

## Quality gate

Run `make check`. It covers formatting, Clippy, all Rust tests, the CLI verifier, the daily-practice use-case script, and the storage-boundary check. Benchmark compilation is `cargo bench --no-run`; benchmark execution is available through the `make bench-*` targets.

## Conventions

Use synchronous storage operations and immediate SQLite transactions. Keep provider details inside `src/storage/`; commands depend on `api::v2` traits. Status is a truth, while observations record the transition trail. Keep tests isolated with temporary `ASOBI_DATABASE_URL` paths and run them serially when they modify process-wide environment variables.

Retention runs **implicitly**: finished sessions and terminal tasks older than `retention_days` (default 7, or `ASOBI_RETENTION_DAYS`, `0` to disable) are deleted once per process, before the first write. Deliberately on a write and not at open, so a read never mutates the graph — but it does mean a test that seeds old finished state and then writes will see it vanish. Set `retention_days = 0` in such a test rather than working around the deletion.
