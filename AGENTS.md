# Asobi

Persistent knowledge-graph CLI for humans and AI agents. Asobi stores entities, observations, truths, relations, and task state in a local SQLite database. Skills are deliberately **not** in that list: they live on the filesystem, managed with the [`skills` CLI](https://github.com/vercel-labs/skills), because the disk copy under `.agents/skills/` is the one agents actually read.

## How to read this file

It records the things that are true about the _shape_ of the project — boundaries, conventions, and the traps that cost someone an afternoon. It deliberately does not enumerate commands, flags, files, or defaults. Every one of those already has a generated or enforced source, named below, and a second hand-maintained copy here would be a copy that goes stale — which is exactly what happened before 0.7.1, when this file still described a 0.6 storage model and advertised a command that had been deleted.

So: when you change the code, ask whether this file states something that is now false, not whether it lists something new. Adding an inventory here is how it rots.

## Where the answers actually live

| Question | Authority |
| --- | --- |
| What commands exist, and their flags | `asobi --help`; the clap definitions in `crates/asobi/src/cli/commands.rs` |
| What a command does, for a user | `docs/usage.md` — the single user-facing reference |
| The machine-readable response contract | `asobi schema`, explained in `docs/response-contract.md` |
| That every subcommand stays reachable | `tests/cli_command_coverage_test.rs` |
| What `make check` covers | the `check` target in `Makefile` |
| Tool and runtime versions | `.mise.toml` (`make init` provisions; CI resolves the same file) |
| Why a past decision was made | `CHANGELOG.md` and `docs/decisions/` |

`docs/architecture.md` explains the design; this file is the working contract.

## Stack and layout

Rust 2024, a four-crate Cargo workspace (`asobi-core`, `asobi-storage`, the `asobi` CLI, `asobi-server` — ADR 0009), Clap, tracing, sqlx (bundled SQLite/FTS5) on tokio, and Python scripts run through `uv`.

The layout worth knowing is the boundary, not the file list:

- `crates/asobi-core` — domain types, the async `api::v3` capability traits, and configuration/path resolution. No I/O stack.
- `crates/asobi-storage` — the SQLite provider on sqlx. Every provider detail stays behind this line, and the boundary is a dependency rule: only this crate lists `sqlx`, which Cargo enforces (`make check` fails if another crate lists it).
- `crates/asobi` — the CLI binary: parsing, routing, and output. The only layer that knows it is a terminal.
- `crates/asobi/src/tasks.rs` — durable task planning, dispatch, sync and close. `crates/asobi/src/compact.rs` — the graph-to-Markdown topic projection.
- `crates/asobi/src/frontmatter.rs` — a deliberately narrow YAML-frontmatter subset, **not** a real parser. Read its module doc before widening it; see the trap below.
- `crates/asobi-server` — the server binary (WP4); a stub until then.
- `tests/`, `benches/` — contract, CLI, edge-case and multi-process verification; graph, SQLite, task, allocation and SQL-plan benchmarks.

Anything not listed is a leaf: find it with `rg`, and it needs no entry here.

## Documentation split

This repository documents what the CLI _is_ and ships no `SKILL.md`. Agent workflow guidance — advice about _when_ to reach for a command — lives in the [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md):

```bash
npx skills add https://github.com/azusachino/harus-skills --skill asobi --agent universal
```

Keep the split when adding documentation. A change to a command's behaviour belongs in `docs/usage.md`. Advice about when to use it does not belong in this repository at all.

## Quality gate

Run `make check` before committing. Benchmarks compile as part of it; they execute only through the `make bench-*` targets.

## Conventions

- Storage operations are async, over sqlx; mutations run in `BEGIN IMMEDIATE` transactions.
- Commands depend on `api` traits, never on a storage type.
- Status is a truth; observations record the transition trail.
- Tests isolate with a temporary `ASOBI_DATABASE_URL`, and run serially when they touch process-wide environment variables.

## Traps

Three things here have bitten someone and will bite again. Each is a deliberate design choice that reads like a bug.

**The sweep runs implicitly, on write.** Abandonment and retention are one sweep, once per process, before the first write — deliberately on a write and not at open, so a read never mutates the graph. Abandonment first: an open task (epics excepted while a `part_of` child is open) idle past `abandon_days` becomes `ABANDONED`, and the abandonment observation refreshes its activity, so a just-abandoned task survives retention for its full window. Then retention: finished tasks past `retention_days` are deleted. The consequence: a test that seeds old state and then writes will watch it vanish or flip. Pin both windows through `ASOBI_ABANDON_DAYS`/`ASOBI_RETENTION_DAYS` (or `0` to disable) rather than working around the sweep. The windows, their environment overrides, and the defaults live in `crates/asobi-storage/src/storage/sqlite.rs` and `crates/asobi-core/src/paths.rs`.

**`crates/asobi/src/frontmatter.rs` is a subset, not YAML.** It handles a flat `key: value` block and nothing else — no nesting, lists, comments, or multi-line scalars. A document declaring `description: >` therefore parses as the literal `">"`. That is known and accepted: the fix is to stop depending on the field, not to widen the parser. That call has already been made once — a block-scalar implementation was written to fix exactly this, then thrown away in favour of dropping the manifest's `description` field, which nothing read. Before widening the subset, check whether the value is load-bearing at all.

**State and project content are anchored separately.** `AsobiPaths::root` is where project content resolves; `data_dir`, `config_dir` and `topics_dir` are state. The two do not move together — under the XDG fallback `root` follows the working directory while the state directories are global. Anything asobi regenerates belongs on the state side, and if it describes something on the project side it has to record _which_ something. The topics directory is the boundary in miniature: it lives on the state side, but each topic's frontmatter names the entity it projects, so a reader can always tell which workspace root a topic belongs to rather than trusting whichever tree it happened to be found under.
