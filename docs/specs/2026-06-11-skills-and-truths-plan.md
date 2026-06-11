# Implementation Plan: Truths tier, lazy reads, observation cap, skills subsystem

Date: 2026-06-11
Spec: [`2026-06-11-skills-and-truths.md`](./2026-06-11-skills-and-truths.md)
Tracked in rosemary as epic `rosemary:skills-truths` (task entities `…:task-N`).

## How to work this plan (for worker agents)

- **TDD, every task**: write the failing test first, watch it fail, implement, watch it
  pass, refactor. No production code without a red test first.
- **Dispatch cadence: sequential.** Tasks share `db.rs` / `constant.rs` / `mcp.rs` /
  `main.rs` — they are *not* disjoint, so do not parallelize in worktrees. One task at a
  time; the lead reviews the diff and commits before the next starts.
- **Per task**: run `make check` (fmt + clippy `-D warnings` + test + scripts) before
  handing back. Record results in the task entity (`impl:` + `status: REVIEW`).
- **Test pattern**: inline `#[cfg(test)]` module in the touched source file, following the
  existing `db.rs` tests — set `ROSEMARY_DATABASE_URL` to a tempfile, `init_db().await`,
  exercise the function. Cross-cutting CLI behavior goes in `tests/`.
- **Fetch your task**: `rosemary open-nodes "rosemary:skills-truths:task-N"` — the `title:`
  observation carries files, plan, and the test to write.

## Dependency order

```
task-1  add-observations multi-arg        (independent)
task-2  truths storage + CRUD + CLI       (foundation)
task-3  observation cap                    (touches add path; after task-2)
task-4  lazy reads + EntityOutput shape    (needs task-2)
task-5a skills storage + open-nodes body   (needs task-4)
task-5b skills install/update/remove/list  (needs task-5a)
task-5c documents-feature body embedding   (needs task-5a; optional, feature-gated)
task-6  MCP tool schema (add_truth etc.)   (needs task-2, task-4)
task-7  docs (usage/AGENTS/CHANGELOG)       (last)
```

---

## task-1 — `add-observations` multi-arg

**Files**: `src/main.rs` (`Commands::AddObservations` at :46, dispatch arm at :307).

**Test first** (`tests/` CLI test or inline): `add-observations foo "a" "b" "c"` results in
three observation rows on `foo`.

**Change**: `content: String` → `contents: Vec<String>` with `#[arg(num_args = 1..)]`; pass
`contents` directly into the existing `ObservationInput { entity_name, contents }`. No DB or
MCP change (the vec already exists).

**Done when**: multi-arg adds N rows in one call; single-arg still works; `make check` green.

---

## task-2 — Truths storage + CRUD + CLI

**Files**: `src/constant.rs` (schema + SQL consts), `src/db.rs` (schema init list + helpers
+ tests), `src/main.rs` (`AddTruth`, `DeleteTruth` subcommands).

**Tests first** (inline in `db.rs`):
- `truth_upsert` twice on the same `(entity, key)` → exactly one row, latest value.
- `truth_upsert` on two keys → two rows.
- `delete_truth` removes one key only.
- `delete-entities` cascades truths (FK `ON DELETE CASCADE`).

**Implement**:
- Schema (from spec): `mcp_truths(entity_name, key, value, updated_at, PRIMARY KEY
  (entity_name, key), FK → mcp_entities ON DELETE CASCADE)`. Add the const to the
  schema-creation sequence in `init_db` (`src/db.rs:9`).
- SQL consts: upsert (`INSERT … ON CONFLICT(entity_name,key) DO UPDATE SET value=…,
  updated_at=CURRENT_TIMESTAMP`), delete-by-key, select-by-entity, select-in-template.
- `db::truth_upsert(conn, entity, key, value)`, `db::truth_delete(conn, entity, key)`,
  `db::select_truths(conn, &[names]) -> HashMap<String, Vec<(String,String)>>`.
- CLI: `rosemary add-truth <entity> <key> <value>`, `rosemary delete-truth <entity> <key>`.
- Truths are **not** FTS-indexed.

**Done when**: upsert/delete/cascade tests pass; CLI works; `make check` green.

---

## task-3 — Observation cap

**Files**: `src/paths.rs` (`RosemaryConfig` at :6 — add `observation_limit: Option<usize>`),
`src/constant.rs` (env const + evict SQL), `src/db.rs` (`mcp_add_observations` at :143).

**Tests first** (inline in `db.rs`):
- With limit 3, inserting 5 observations leaves exactly the newest 3 (oldest evicted by
  `rowid`).
- Limit `0` → unbounded (no eviction).
- Count after eviction reflects the cap.

**Implement**:
- Resolution order: `RosemaryConfig.observation_limit` → env `ROSEMARY_OBSERVATION_LIMIT`
  → default `50`. Add `ROSEMARY_OBSERVATION_LIMIT` const; thread the resolved limit into
  `mcp_add_observations`.
- In the existing insert transaction (`mcp_add_observations` already uses a tx), after
  inserting, if limit > 0 delete oldest rows for that entity beyond the limit:
  `DELETE FROM mcp_observations WHERE entity_name = ?1 AND rowid NOT IN
   (SELECT rowid FROM mcp_observations WHERE entity_name = ?1 ORDER BY rowid DESC LIMIT ?2)`.
  Must run inside the same tx so the cap is never transiently exceeded.

**Note**: the FTS delete trigger (`mcp_obs_ad`) keeps `mcp_obs_fts` in sync on eviction —
verify the trigger fires for the cap delete (it should; it's `AFTER DELETE`).

**Done when**: cap tests pass; `0` disables; FTS stays consistent; `make check` green.

---

## task-4 — Lazy reads + `EntityOutput` shape

**Files**: `src/mcp.rs` (`EntityOutput` at :62, `Graph` at :70), `src/db.rs`
(`mcp_read_graph` :235, `mcp_search_nodes_with_limit` :284, `mcp_open_nodes` :359).

**Tests first** (inline in `db.rs`):
- `read-graph` / `search-nodes` on an entity with truths + observations returns the truths
  and `observation_count`, and **no** observation content.
- `open-nodes` on the same entity returns truths **and** all observation content.

**Implement**:
- `EntityOutput`: add `truths: Vec<(String,String)>` (always), `observation_count: usize`
  (always), make `observations: Vec<String>` `#[serde(skip_serializing_if =
  "Vec::is_empty")]` (populated only by open-nodes). (Body field arrives in task-5a.)
- `mcp_read_graph` / `mcp_search_nodes_with_limit`: stop selecting observation content;
  select `COUNT(*)` per entity + JOIN truths.
- `mcp_open_nodes`: select truths + all observation content.
- Update any in-repo callers of `EntityOutput.observations` (e.g. `compact.rs:36`,
  `backup.rs` import path) to the new shape — they use `open-nodes`/`read-graph` data;
  ensure they read from the eager path or tolerate empty `observations`.

**Done when**: lazy vs eager tests pass; callers compile; `make check` green.

---

## task-5a — Skills storage + open-nodes body

**Files**: `src/constant.rs` (`mcp_skills` schema + SQL), `src/db.rs` (helpers +
`mcp_open_nodes` body attach), `src/mcp.rs` (`EntityOutput.body`).

**Tests first** (inline in `db.rs`, no git — synthesize rows):
- `skill_upsert` then `open-nodes` returns the body; a second `skill_upsert` replaces it.
- `read-graph` / `search-nodes` never return the body.
- `delete-entities` cascades the skill body row.
- `list_skills` returns name + description (truth) + version, grouped by source.

**Implement**:
- Schema: `mcp_skills(entity_name PK, body, source, version, installed_at, FK →
  mcp_entities ON DELETE CASCADE)`.
- `EntityOutput.body: Option<String>` `#[serde(skip_serializing_if = "Option::is_none")]`.
- `db::skill_upsert(conn, entity, body, source, version)`, `db::skill_body(conn, entity)`,
  `db::list_skills(conn) -> Vec<SkillRow>`.
- `mcp_open_nodes`: LEFT JOIN `mcp_skills`; set `body` when present.

**Done when**: body is eager-only, cascades, listed; `make check` green.

---

## task-5b — Skills install / update / remove / list

**Files**: new `src/skills.rs`, `src/lib.rs` (module), `src/main.rs` (`Skills` subcommand
group), `Cargo.toml` (move `walkdir` to base deps).

**Tests first**:
- Frontmatter parse: valid (`name`/`description` extracted), missing frontmatter (skipped),
  malformed (skipped with warning). Pure function, unit-tested.
- Source-slug derivation: `https://github.com/jasonswett/llm-skills[.git]` →
  `jasonswett-llm-skills`.
- Entity naming: `skill:<slug>:<name>`.
- Selection resolution: `--all` selects all; `--select a b` selects named; neither + no TTY
  → error. (Inject a `is_tty`/selection seam so this is unit-testable without a terminal.)
- Git clone + walk behind an `#[ignore]` integration test (network) OR a local fixture repo
  created with `git init` in a tempdir.

**Implement**:
- `rosemary skills` (no subcommand) → `list_skills`, grouped by source: `skill:<slug>:<name>
  · <description> · <version>`. Lazy.
- `rosemary skills install <source> [--all|--select <name>...]`:
  1. `git clone --depth 1 <source>` → tempdir (shell out; bail with context on failure).
  2. Walk `*.md` (walkdir), parse frontmatter.
  3. Resolve selection (TTY multiselect, else `--all`/`--select`, else error).
  4. `git rev-parse HEAD` for the version SHA.
  5. Per chosen skill, one transaction: upsert entity (`type:"skill"`) + truths
     (`description`, `source`, `version`, `installed`) + `mcp_skills` row.
  6. Remove tempdir (RAII guard so it cleans on error too).
- `rosemary skills update [source]`: re-clone (all sources, or the named slug/url), refresh
  body + bump `version` truth and row.
- `rosemary skills remove <name|source>`: delete entity by fully-qualified name, or all
  skills under a source slug (body cascades).

**Done when**: parse/slug/naming/selection unit tests pass; install→`open-nodes` round-trips
a skill body; `make check` green (base build, no `documents`).

---

## task-5c — Documents-feature body embedding (optional)

**Files**: `src/skills.rs` (feature-gated), `src/ingest.rs`/`src/vector.rs` reuse.

**Tests first**: with `--features documents`, after install, `rosemary query "<skill topic>"`
surfaces the skill.

**Implement**: behind `#[cfg(feature = "documents")]`, chunk + embed skill bodies into the
existing vector store on install/update (reuse `chunk` + `embed` + `vector`). Purely
additive; base build unaffected.

**Done when**: `make test-documents` covers it; base build unchanged.

---

## task-6 — MCP tool schema updates

**Files**: `src/mcp.rs` (tool list JSON ~:240, `run_server` dispatch ~:389, params structs).

**Tests first** (`tests/verbose_tool_test.rs` style): `add_truth` MCP tool upserts; tool
list advertises `add_truth`/`delete_truth`; output objects carry `truths`/`observationCount`.

**Implement**: add `add_truth` + `delete_truth` tools wired to `db::truth_upsert`/
`truth_delete`; update the `add_observations` description to mention the cap; ensure
serialized entity output matches the new `EntityOutput` shape.

**Done when**: MCP tool tests pass; `make check` green.

---

## task-7 — Docs

**Files**: `docs/usage.md`, `AGENTS.md` (CLI surface), `docs/CHANGELOG.md`,
`docs/architecture.md` (truths/observations/skills model + lazy reads).

Update the CLI surface lists, document `add-truth`/`delete-truth`/`skills *`, the lazy-vs-
eager read contract, and the observation cap config. **Out of repo** (note only): the
rosemary `SKILL.md` plugin workflows should be updated to write session/task state as
truths and to fetch skills via `open-nodes` — flag for a follow-up in the plugin repo.

**Done when**: docs reflect reality; `make check` green.

---

## Epic close

When all tasks are `DONE`, promote durable lessons into the `rosemary` project entity
(e.g. "session/task state is truths, not observations"; "observations are capped append-log
history"), mark `rosemary:skills-truths` `status: DONE`, and leave it queryable.
