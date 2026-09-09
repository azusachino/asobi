# Changelog

## v0.7.0 — Skills leave the graph

Over a third of Asobi was a skill installer: `skills.rs`, `cli/skills.rs` and `skills_config.rs` came to 1,798 of 4,985 lines. It stored every skill twice — once as a graph entity carrying a body, once on disk — and the disk copy was the one agents actually read, because `.agents/skills/` is what the Agent Skills ecosystem understands and what `rg` reaches. Measured against a real six-week-old graph, the graph copy had accumulated nothing: all 33 installed skills had zero observations and a lone `description` truth.

So the filesystem becomes the only copy.

### Breaking

- **Truth history is gone** — the `asobi_truth_history` table, the `history` command, and the archive-on-overwrite write path. It was the one store with no bound of its own: observations are capped per entity and current truths are one row per key, but every overwrite appended a version that survived until its entity was deleted. On a six-week-old graph that was 616 rows, 496 of them sessions whose `next` had been rewritten 139 times.

  It had no reader. `asobi history` appeared in no workflow, and where a trail genuinely mattered the observations already carried it in better form: a task's history held one row saying `status=DISPATCHED`, beside an observation saying "dispatched to codex". A bi-temporal store answers questions about how state changed over time, and nothing here asked one. Schema 7 drops the table.

- **Finished sessions and tasks are deleted after 7 days**, automatically, once per process before the first write — on a write rather than at open, so a read never mutates the graph. Configurable via `retention_days` in `asobi.toml` or `ASOBI_RETENTION_DAYS`; `0` disables it. The previous manual `purge` was correct in every respect except that it never ran, leaving a graph that was 96% finished work.
- **`purge` takes two flags instead of five.** `--type` and `--status` are gone: they were validated against a fixed set, so they could only ever select a subset of the single policy — configuration that could not express anything new. `--dry-run` is gone too; it only ever restated the default. What remains is `--older-than` and `--apply`. "Purge refuses durable knowledge" is now structural rather than enforced, since no request can name a durable type.

- **Skills are no longer graph entities.** They do not appear in `graph`, `search`, or `show`. Search one with `rg` over the skills directory.
- **`show` no longer returns `body`.** The field existed only to carry skill bodies and is gone from the response contract; `asobi schema` reflects this.
- **Schema is now 6.** Upgrading drops the `asobi_skills` table and deletes `skill`-typed entities rather than leaving them as husks with no body — cascades take their truths, observations and relations. Nothing there was the only copy: the bodies are on disk, and `skills sync` rewrites that tree from `asobi.toml` regardless. The upgrade runs `PRAGMA incremental_vacuum` so the file shrinks with the graph.
- **Reference inlining is gone**, replaced by the Markdown copy above. Inlining (0.6.3) folded a skill's local `.md` references into its stored body, which inverted the specification's progressive disclosure — a deliberately tiered skill became a monolith, at exactly the token multiple the tiering exists to avoid — and only ever covered markdown, so scripts and assets vanished regardless.

  Measured against `mattpocock/skills`: `tdd` keeps its authored 38-line body with `mocking.md` and `tests.md` beside it, loaded on demand, instead of a 138-line blob. And `diagnosing-bugs`, whose body instructs the agent to run `scripts/hitl-loop.template.sh`, previously installed _without that script_ — inlining could not carry it. It now ships.

- **A skill is a directory containing `SKILL.md`, and nothing else is accepted.** Loose `<name>.md` files were installable before, which bought a filename-based name fallback, an optional bundle threaded through collection and materialisation, and a class of skill whose relative references could never resolve once installed — three kinds of complexity for a shape the specification does not define. A source with no `SKILL.md` now fails with a message saying so. `src/skills.rs` is 1,260 → 1,038 lines across this release.

- **`SkillStore` and `SkillRecord` are removed** from `api::v2`. Library consumers implementing the trait no longer need to; there is no storage-side skill surface at all.

### Added

- **Skill provenance on disk.** `skills sync` writes `.asobi-skills.json` beside the installed skills, recording each one's source and the exact commit it came from, and `asobi skills` reports that commit. This replaces source/version truths that the graph nominally held and in practice never populated — no installed skill had a version recorded. Committing the manifest with the skill files is what makes an upstream skill change a reviewable diff, which matters because a skill is natural-language instruction loaded straight into an agent's context.
- **A skill's referenced Markdown is installed with it.** Previously only `SKILL.md` was written, so a skill pointing at `references/REFERENCE.md` installed "successfully" and was broken the moment an agent followed the link. The rest of the directory is _not_ copied: `scripts/`, `assets/` and tool-specific config are fetched from a git URL and are exactly where the published attack research finds payloads hidden, precisely because scanners read the body and not the artifacts beside it. Writing an upstream executable to disk on the strength of a `select` line is a bigger promise than a skill installer should make. Skipped files are named in a warning rather than dropped silently, and Markdown is refreshed on every sync since an upstream change to a reference need not change `SKILL.md`. Symlinks are not followed.
- **`[[skills.source]]` accepts `rev`, and `skills install` accepts `--rev`.** Pin a source to a commit, tag, or branch instead of following its default branch. Without it a re-sync silently adopts whatever the source moved to, which for natural-language instructions loaded into an agent's context is an unreviewed behaviour change rather than a dependency bump. An annotated tag resolves to the commit it points at, so the recorded version is always a commit.
- **`make check` validates Asobi's skill output against the Agent Skills specification**, installing a fixture with the built CLI and running the reference validator published alongside the spec over what lands on disk — including that bundled resources survived. One divergence is deliberate and documented: the spec requires a skill's directory name to equal its frontmatter `name`, which assumes single-source authoring, while Asobi installs many sources into one tree as `<source-slug>@<skill-name>` — two sources may ship the same name, and agent hosts surface the directory name as the skill's identity. Each skill's content is validated under a conformant name, so everything else the spec requires is enforced.
- **`asobi init --local` scaffolds a commented `[skills]` block.** It names no source: scaffolding aids discovery of `skills sync`, but defaulting to any particular skill repository would install instructions the user never asked for.

- **`tasks list` answers "what is open".** Without an epic it now returns only unfinished work; `--all` restores the full board. Measured on a real graph the unfiltered form was 1,972 lines of JSON that was 96% completed tasks, which is too expensive to be what an agent runs at session start — exactly when it is wanted. It is 58 lines filtered. An entity with no `status` truth counts as open, which is what surfaces an epic whose children are all `DONE` but which was never closed: it appears alone, with no open children under it. Three epics on the measured graph were in that state and nothing reported them.
- **`tasks sync` and `tasks close` record `commit` and `branch` truths** when run inside a git worktree. A checkpoint that carries status and a next action but not the revision it was true at cannot support validated continuation — the successor knows what to do and not what tree to do it against. No entity in the measured graph carried either. Captured automatically rather than behind a flag, since an optional field on a handoff is empty exactly when the handoff matters; a detached `HEAD` records the commit and no branch, and running outside a repository records neither, neither being an error.

- **`show` returns the most recent observations, not the whole trail.** `--limit` defaults to 20; `--limit 0` restores the full read, which is what `export` uses. `observationCount` still reports the true total, so a caller can always tell what it is not being shown. `graph` and `search` were already lean and `show` was the one unbounded eager read: on a real graph, loading `iroha:session` at 156 observations cost **157 KB**, and the session-start read the asobi skill prescribes cost **180 KB** — roughly 45,000 tokens to answer "where was I". They are now 26 KB and 48 KB.

- **Search reaches truth values.** A third FTS index covers `asobi_truths`, so an entity is findable by what its truths say. This was a real hole: the convention is to store a pitfall's human-readable warning in a `title` truth, which made the one sentence explaining a dead end the one thing recall could not reach. Verified against a real graph — the pitfall whose title reads "bump the Valkey generation manually" was unfindable by any word in it.
- **A multi-word query widens instead of failing closed.** FTS5 ANDs bare terms, so a natural-language question whose words are spread across an observation, a truth and a name matched nothing — and an empty result is indistinguishable from "nothing was ever recorded", which is the wrong way for a pitfall lookup to fail. `search` now retries with `OR` and reports the widening on stderr.
- **Search results come back ranked.** The three retrieval paths are combined with reciprocal rank fusion rather than concatenated, over a fixed candidate pool so ranking does not shift with `--limit`.

### Fixed

- **`search` was returning results in alphabetical order, never relevance order.** The entity fetch ended in `ORDER BY name`, which re-sorted the result set and discarded whatever ranking search had just computed — for every release that has shipped. `show` now also returns entities in the order they were asked for. On a real graph, the pitfall answering "deploy without cache bump" went from _no results at all_ to third.
- **`skills` and `skills show` work under a plain `asobi init`.** The skills directory now falls back to `.agents/skills` under the discovered root when no `asobi.toml` declares a `[skills]` block. Previously every skills path assumed a project-local config, but `asobi init` without `--local` writes none — so the default XDG install had no reachable skills directory.
- **`skills install` and `skills update` no longer disturb other sources.** Both rewrite the tree, so they now carry unaffected sources through untouched; `--all` remains a full sync of its own source, and a scoped `update <source>` leaves siblings alone.

### Documentation

- `SKILL.md` removed from this repository. It had drifted into contradicting the maintained [`asobi` skill](https://github.com/azusachino/harus-skills/blob/main/skills/asobi/SKILL.md) — making `compact` a session-end step the skill warns against, storing the handoff as an observation where the skill stores a truth, and never mentioning the task dispatcher. This repository documents what the CLI _is_; workflow guidance lives in the skill. `docs/usage.md` is now the single command reference, and `docs/workflow.md`'s narrative walkthrough moved to harus-kb.
- Correcting `docs/usage.md` against `asobi <command> --help` turned up four documented behaviours that do not exist: `search --limit` defaults to **10**, not the 100 claimed in four places (the 100 is a storage-layer fallback for a limit of 0, which the clap default never produces); `rm-obs` takes `--id`, not the documented `--prefix`, and accepts one observation rather than a list; `update-obs --id` was undocumented; and `capabilities` and `reset` were absent from the reference entirely.
- `AGENTS.md` now holds the project conventions, with `CLAUDE.md` as an `@AGENTS.md` pointer, so every agent runtime reads the same contract.

### Verification

`make check` passes: storage boundary, rustfmt, Prettier, Ruff, Clippy `-D warnings`, all Rust tests, CLI verifier, use cases, and benchmark compilation. The schema 6 upgrade was exercised against this workstation's own project-local graph — 1 skill entity to 0, schema 5 to 6, manifest written with the resolved commit — after a backup, not only against the synthetic fixture. New coverage: `test_materialize_prunes_skills_dropped_upstream`, `test_manifest_records_provenance`, and `test_read_installed_falls_back_to_scanning`.

## v0.6.4 — Physical storage reclamation

### Fixed

- A database that has existed since before the 0.6 rusqlite rewrite carries every superseded schema generation's tables in place — the original `mcp_*` schema, then the libSQL/Turso-era `chunks`/`topics` vector schema — because each rewrite only ever added its own tables and never dropped the ones it replaced. Combined with SQLite's default `auto_vacuum=NONE`, a long-lived database could be over 95% dead pages that no command ever targeted. Schema v5 drops those tables on upgrade and switches every database to `auto_vacuum=INCREMENTAL` (a one-time `VACUUM` for an upgrading database, the pragma alone for a fresh one); `purge --apply` now runs a bounded `PRAGMA incremental_vacuum` afterward so routine purges keep reclaiming space instead of only marking it free. See ADR 0003 for the full account, including why this was previously deferred.
- `PRAGMA auto_vacuum` only takes effect before a database's file header is first written, which `PRAGMA journal_mode=WAL` does as a side effect. `open_at` was setting `auto_vacuum` after switching to WAL, so it silently never took effect on a fresh database; the pragma now runs first.
- `compact --older-than` claimed to prune session Markdown files, but nothing has written to `.asobi/topics/sessions/` since sessions were excluded from the Markdown projection — the flag was dead code from before that exclusion. Removed; `compact` is sync-only now.

### Tooling

- Toolchain pins bumped: Rust 1.98.0, uv 0.12.5, bun 1.4.0, ruff 0.16.4; `criterion` to 0.8. Rust 1.98's clippy adds `chunks_exact_to_as_chunks`, which `new`/`link`'s pair/triple parsing now satisfies via `as_chunks`.

### Verification

Added `opening_a_pre_v5_database_drops_superseded_tables_and_enables_incremental_vacuum` and `applied_purge_reclaims_space_via_incremental_vacuum` to the backend contract suite; both caught the WAL-ordering bug above before it shipped. The physical shrink itself (60MB → 1.2MB, 96% of pages reclaimed) was verified against a real long-lived database copy, not just the synthetic fixture. `make check` passes: storage boundary, rustfmt, Prettier, Ruff, Clippy `-D warnings`, all Rust tests, CLI verifier, use cases, and benchmark compilation.

## v0.6.3 — Self-contained skills and reliable releases

### Added

- Skill install/sync now inlines a skill's local `.md`/`.markdown` references into its stored body, so a `SKILL.md` that is itself just a table of contents over sibling docs ships self-contained. Both markdown links (`[text](path)`) and backtick-quoted paths (`` `references/schemas.md` ``, the style Anthropic's own `skill-creator` uses) are followed. A reference to another skill's own entry point (`SKILL.md`/`index.md`) is never inlined — that stays a cross-skill reference to something installed as its own entity — and a link resolving outside the source checkout is never followed.
- `--subdir <path>` on `skills install`, and a matching `subdir = "..."` field on `[[skills.source]]` in `asobi.toml`, scope the install walk to one directory of a checkout. Some sources mirror every skill across several tool-specific directories (`.opencode/`, `.kiro/`, a canonical `skills/`, ...) with the same `name:` in each copy; `subdir` avoids the mirrors entirely instead of asking install to arbitrate between diverging copies.

### Fixed

- A source that declares the same skill `name:` in more than one file (a real pattern: mirrored copies under different tool-specific directories) used to fail with a confusing `Content missing for skill X`. It now fails with a specific error naming every colliding file, and — for `--select` — only when the actually-selected name collides, so an unrelated, unambiguous skill in the same source still installs.
- The release workflow's macOS binary upload had no retry, so a transient connect-timeout to `api.github.com` (observed directly in CI logs, alongside `mise-action` hitting the same timeout independently) failed the whole release. The GitHub release is now created once in `publish-crate`, and each platform's binary upload retries with backoff — `v0.6.2`'s release shipped without its macOS binary because of exactly this.

### Verification

Reference inlining and the duplicate-name fix were verified against real upstream skill repos (`mattpocock/skills`, `obra/superpowers`, `anthropics/skills`, `DietrichGebert/ponytail`, `addyosmani/agent-skills`, `jasonswett/llm-skills`), not just synthetic fixtures. `make check` passes: storage boundary, rustfmt, Prettier, Ruff, Clippy `-D warnings`, all Rust tests, CLI verifier, use cases, and benchmark compilation.

## v0.6.2 — Declarative skill sync

### Added

- `asobi skills sync` reconciles installed skills with a `[skills]` block in the discovered `asobi.toml`, treating the config as the whole truth: declared sources are installed or refreshed, unselected skills are pruned, and skills from undeclared sources are removed.
- Selected skills are materialised to disk at `<path>/<source-slug>@<skill-name>/SKILL.md` alongside the graph copy, so agents can read them off the filesystem. Both halves of the directory name are slugified to lowercase kebab-case. `path` defaults to `.agents/skills`, relative to the declaring `asobi.toml`. On-disk pruning is scoped to the `@` naming convention, leaving vendored checkouts and hand-authored skills untouched.

### Changed

- `AsobiPaths` now carries the discovered workspace `root` and `config_file`, so project-relative content paths resolve consistently regardless of the working directory.
- `install_skills_from_dir` reports what it installed and pruned instead of returning unit.

### Tooling

- Tool versions (rust, uv, bun, ruff) are pinned in `.mise.toml`; `make init` provisions them and both workflows resolve the same file, so local and CI no longer drift. `flake.nix` and `flake.lock` are removed.
- `[tool.ruff]` is declared rather than inherited: without it ruff walks up out of the repo and adopts a parent directory's config, which reformatted the tree to width 100 when checked out inside such a workspace. Ruff 0.16 also widens the default rule set from 118 rules to 826.

### Verification

`make check` passes, including storage-boundary checks, Clippy, Rust tests, CLI integration checks, use cases, and benchmark compilation.

## v0.6.1 — Lean agent reads and safe retention

### Added

- Preview-first `purge` for stale terminal `session` and `task` entities, with transactional deletion, relation/FTS cleanup, JSON reports, and durable-knowledge safeguards.
- Shell completion generation for Bash, Elvish, Fish, PowerShell, and Zsh through `asobi completions <shell>`.

### Changed

- `graph` and `search` now return lean entity indexes: observations and skill bodies are lazy and available through explicit `show`, `export`, `backup`, or `skills show` operations.
- Updated the usage guide and retention ADR with the 0.6.1 maintenance and completion workflows.

### Verification

`make check` passes, including storage-boundary checks, Clippy, Rust tests, CLI integration checks, use cases, and benchmark compilation. Tarpaulin reports 62.60% line coverage (1,053/1,682).

## v0.6.0 — Curated SQLite graph storage

### Added

- Synchronous `api::v2` storage traits for graph, search, skills, snapshots, backups, maintenance, and task dispatch.
- Bundled SQLite through `rusqlite`, with WAL mode, foreign keys, bounded busy timeouts, and FTS5/BM25 keyword search.
- Durable task dispatcher: `tasks plan`, `list`, `dispatch`, `sync`, and `close` with nested help, lifecycle validation, and JSON response schemas.
- Atomic task dispatch: status transition, claimant truth, and dispatch observation commit together, so concurrent agents produce one winner.
- Graph-to-Markdown `compact` projection for durable knowledge topics.
- Contract, CLI, evil-input, edge-case, concurrent-process, daily-practice, and benchmark coverage.

### Removed

- libSQL/Turso and SQLx providers.
- Vector/document ingestion, semantic recall, and feature-gated product paths.
- The obsolete async v1 storage contract and provider-specific verification scripts.

### Verification

`make check` runs formatting, Clippy, all Rust tests, the CLI verifier, the daily use-case scenario, and storage-boundary checks. `cargo bench --no-run` verifies all benchmark targets; `make bench` executes them.

## v0.5.2 — Versioned CLI responses

- Added command-specific JSON Schema discovery through `schema` and `schema --command NAME`.
- Standardized structured errors and local-time tracing output.

## v0.5.1 — Leaner CLI build

- Reduced default CLI dependencies and tightened logging and formatting gates.

## v0.5.0 and earlier

- Established the standalone knowledge-graph CLI, SQLite-compatible graph schema, truths, observation history, lazy reads, skills, compact Markdown projections, portable JSON export/import, and local/XDG workspace layouts.
