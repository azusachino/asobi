# Asobi

Persistent knowledge-graph CLI for humans and AI agents. Asobi stores entities, observations, truths, relations, and task state in a local SQLite database. Skills are deliberately **not** in that list: they live on the filesystem, because the disk copy under `.agents/skills/` is the one agents actually read.

## How to read this file

It records the things that are true about the _shape_ of the project — boundaries, conventions, and the traps that cost someone an afternoon. It deliberately does not enumerate commands, flags, files, or defaults. Every one of those already has a generated or enforced source, named below, and a second hand-maintained copy here would be a copy that goes stale — which is exactly what happened before 0.7.1, when this file still described a 0.6 storage model and advertised a command that had been deleted.

So: when you change the code, ask whether this file states something that is now false, not whether it lists something new. Adding an inventory here is how it rots.

## Where the answers actually live

| Question | Authority |
| --- | --- |
| What commands exist, and their flags | `asobi --help`; the clap definitions in `src/cli/commands.rs` |
| What a command does, for a user | `docs/usage.md` — the single user-facing reference |
| The machine-readable response contract | `asobi schema`, explained in `docs/response-contract.md` |
| That every subcommand stays reachable | `tests/cli_command_coverage_test.rs` |
| What `make check` covers | the `check` target in `Makefile` |
| Tool and runtime versions | `.mise.toml` (`make init` provisions; CI resolves the same file) |
| Why a past decision was made | `CHANGELOG.md` and `docs/decisions/` |

`docs/architecture.md` explains the design; this file is the working contract.

## Stack and layout

Rust 2024, Clap, tracing, rusqlite with bundled SQLite/FTS5, and Python scripts run through `uv`.

The layout worth knowing is the boundary, not the file list:

- `src/api/` — backend-neutral, synchronous capability traits. Commands depend on these.
- `src/storage/` — the SQLite provider. Every provider detail stays behind this line, and `make check` enforces it (`scripts/verify_storage_boundary.py`).
- `src/cli/` — parsing, routing, and output. The only layer that knows it is a terminal.
- `src/skills.rs`, `src/skills_config.rs` — skills on disk, and the declarative `[skills]` block `skills sync` reconciles against.
- `src/tasks.rs` — durable task planning, dispatch, sync and close. `src/compact.rs` — the graph-to-Markdown topic projection.
- `src/frontmatter.rs` — a deliberately narrow YAML-frontmatter subset, **not** a real parser. Read its module doc before widening it; see the trap below.
- `tests/`, `benches/` — contract, CLI, edge-case and multi-process verification; graph, SQLite, task, allocation and SQL-plan benchmarks.

Anything not listed is a leaf: find it with `rg`, and it needs no entry here.

## Documentation split

This repository documents what the CLI _is_ and ships no `SKILL.md`. Agent workflow guidance — advice about _when_ to reach for a command — lives in the [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md):

```bash
asobi skills install https://github.com/azusachino/harus-skills.git --select asobi
```

Keep the split when adding documentation. A change to a command's behaviour belongs in `docs/usage.md`. Advice about when to use it does not belong in this repository at all.

## Quality gate

Run `make check` before committing. Benchmarks compile as part of it; they execute only through the `make bench-*` targets.

## Conventions

- Storage operations are synchronous, over immediate SQLite transactions.
- Commands depend on `api` traits, never on a storage type.
- Status is a truth; observations record the transition trail.
- Tests isolate with a temporary `ASOBI_DATABASE_URL`, and run serially when they touch process-wide environment variables.

## Traps

Three things here have bitten someone and will bite again. Each is a deliberate design choice that reads like a bug.

**Retention runs implicitly, on write.** Finished sessions and terminal tasks past the retention window are swept once per process, before the first write — deliberately on a write and not at open, so a read never mutates the graph. The consequence: a test that seeds old finished state and then writes will watch it vanish. Disable retention in that test rather than working around the sweep. The window, its environment override, and the default live in `src/storage/sqlite.rs` and `src/paths.rs`.

**`src/frontmatter.rs` is a subset, not YAML.** It handles a flat `key: value` block and nothing else — no nesting, lists, comments, or multi-line scalars. A skill declaring `description: >` therefore parses as the literal `">"`. That is known and accepted: the fix is to stop depending on the field, not to widen the parser. That call has already been made once — a block-scalar implementation was written to fix exactly this, then thrown away in favour of dropping the manifest's `description` field, which nothing read. Before widening the subset, check whether the value is load-bearing at all.

**State and project content are anchored separately.** `AsobiPaths::root` is where project content resolves; `data_dir`, `config_dir`, `topics_dir` and `cache_dir` are state. The two do not move together — under the XDG fallback `root` follows the working directory while the state directories are global. Anything asobi regenerates belongs on the state side, keyed by what it describes if the thing it describes lives on the other side. The skills manifest is the worked example: it sits in `data_dir`, named for the skills directory it records, because the skills directory itself is project content.
