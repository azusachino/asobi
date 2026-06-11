# Design: Truths tier, lazy reads, and the skills subsystem

Date: 2026-06-11
Status: Approved (design) — pending implementation plan

## Summary

Three related changes, smallest → largest:

1. **`add-observations` multi-arg** — accept many observation strings in one CLI call.
2. **Truths tier + lazy reads** — split an entity's content into *truths* (small canonical
   state, keyed upsert) and *observations* (unbounded append-only log). `read-graph` /
   `search-nodes` become lazy (truths + an observation count); `open-nodes` stays eager
   (truths + full observation log). This is a storage-layer change, motivated below.
3. **Skills subsystem** — `asobi skills install|update|remove` plus `asobi skills`
   (list). Skills are markdown files installed from a git repo into the DB; agents fetch
   them dynamically via `open-nodes skill:<source>:<name>`.

Parts 2 and 3 are designed together because the skills feature is the first real consumer
of both the truths tier (for skill metadata) and a separate body store (for skill bodies).

## Motivation

In practice, agents create a near-empty entity and append *many* observations to it (a
session's running notes, an epic's `status:` audit trail). Two distinct "heavy" problems
result, on different axes:

- **Axis A — many small rows.** A fat entity dumps every observation on every
  `read-graph` / `search-nodes`. This is the dominant real-world cost.
- **Axis B — one large blob.** A skill body is a single multi-KB markdown document; we
  don't want it in keyword search output or the FTS index.

A flat `mcp_observations` list treats canonical state, append-only history, and large
blobs identically, which is why "lazy" reads are hard today. The fix is to model the
three roles separately:

| Role | Store | Cardinality | Mutability | Lazy read | Eager read |
| --- | --- | --- | --- | --- | --- |
| current canonical state | `mcp_truths` | bounded by distinct key | upsert by key | ✅ returned | ✅ |
| append-only log / history | `mcp_observations` | capped at N (evict oldest) | append / delete | ❌ (count only) | ✅ all |
| large blob (skill body) | `mcp_skills` | one per skill | replace | ❌ | ✅ (on open) |

Truths bound the lazy view *by construction* (keyed upsert can't grow unbounded), so no
recency cap, truncation, or `--full` flag is needed. The dispatcher's "latest `status:`
wins" rule becomes a single `status` truth — always current, always in the lazy view —
while the transition history stays as observations (the audit trail).

### Worked example — session save (validates the model)

Observed real Claude Code usage today opens with a `delete-entities` + `create-entities`
dance purely to stop observations accumulating across sessions, then writes six
`key: value` observations (`objective`, `status`, `completed`, `remaining`, `next`,
`last-updated`). Every line is canonical state, and the delete/recreate is a hand-rolled
upsert. Truths replace the whole pattern:

```bash
P="harus-nix"
asobi add-truth "$P:session" objective    "cleanup pass — …"
asobi add-truth "$P:session" status       "DONE"
asobi add-truth "$P:session" completed    "dropped mise-tasks…; committed 7c28260"
asobi add-truth "$P:session" remaining    "none — working tree clean…"
asobi add-truth "$P:session" next         "none pending"
asobi add-truth "$P:session" last-updated "2026-06-11"
```

No delete, no recreate, no accumulation; next session overwrites in place. The session
entity *is* a truths record.

## Part 1 — `add-observations` multi-arg

`ObservationInput.contents` is already `Vec<String>` (`src/mcp.rs:50`) and the MCP path
already accepts an array. Only the CLI surface is single-valued.

- `src/main.rs:46`: `AddObservations { name: String, content: String }`
  → `AddObservations { name: String, contents: Vec<String> }` (clap trailing varargs,
  `num_args = 1..`).
- `src/main.rs:307`: pass `contents` straight into the existing `ObservationInput`.

No DB or MCP change. `asobi add-observations foo "a" "b" "c"` adds three rows in one
transaction.

## Part 2 — Truths tier + lazy reads

### Schema

New table (added to `src/constant.rs` and the schema-init list in `src/db.rs`):

```sql
CREATE TABLE IF NOT EXISTS mcp_truths (
    entity_name TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (entity_name, key),
    FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
);
```

Upsert:

```sql
INSERT INTO mcp_truths (entity_name, key, value) VALUES (?1, ?2, ?3)
ON CONFLICT(entity_name, key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP;
```

Truths are **not** FTS-indexed initially. `search-nodes` matches over observations + entity
name/type as today; truths are display payload, not a new search axis. (If truth search is
wanted later, add an FTS table mirroring `mcp_obs_fts`.)

### CLI

- `asobi add-truth <entity> <key> <value>` — upsert one truth.
- `asobi delete-truth <entity> <key>` — remove one truth.
- Truths are shown wherever an entity is rendered (see output shape below).

`<key>` is a short identifier (`status`, `description`, `objective`, `next`). `<value>` is
free text.

### Output shape

`EntityOutput` (`src/mcp.rs:62`) gains two always-present fields and one optional:

```rust
pub struct EntityOutput {
    pub name: String,
    pub entity_type: String,
    pub truths: Vec<(String, String)>,        // or a serde map; always returned
    pub observation_count: usize,             // always returned
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub observations: Vec<String>,            // populated only by open-nodes (eager)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,                 // skills only, open-nodes only
}
```

### Read behavior

| Command | truths | observation_count | observations | body |
| --- | --- | --- | --- | --- |
| `read-graph` | ✅ | ✅ | — | — |
| `search-nodes` | ✅ | ✅ | — | — |
| `open-nodes` | ✅ | ✅ | ✅ all | ✅ if skill |

Implementation: `mcp_read_graph` / `mcp_search_nodes_with_limit` stop joining
`mcp_observations` content (they SELECT a `COUNT(*)` per entity instead) and JOIN
`mcp_truths`. `mcp_open_nodes` additionally selects observation content and LEFT JOINs
`mcp_skills` for the body.

### Observation cap

Observations are the only unbounded store. Even though lazy reads hide them, they bloat
storage and pollute context on `open-nodes`. Enforce a **hard per-entity cap**:

- On insert, after adding, delete the oldest rows for that entity beyond the limit
  (`ORDER BY rowid ASC`, keep newest N). The newest N always survive, so the latest
  activity is intact; ancient history falls off the back.
- Limit is configurable — default **50**. Resolution order: env (`ASOBI_OBSERVATION_LIMIT`)
  → `asobi.toml` (`observation_limit`) → default.
- A value of `0` means unbounded (opt out).
- Because truths hold current state, the evicted tail is disposable. When the `documents`
  feature is on, `compact` may archive an entity's observations to markdown before
  eviction; without it, eviction is a plain delete.

This runs inside the same transaction as the insert, so the cap is never transiently
exceeded.

### Migration / compatibility

- This changes `read-graph` / `search-nodes` JSON output (observations no longer inline;
  new `truths` / `observationCount` fields). Acceptable at `0.5.0`.
- Existing entities have zero truths until written — they render with an empty `truths`
  array and a correct `observationCount`. No data migration required.
- Downstream: the asobi **skill** (`SKILL.md` workflows) and the **MCP tool schemas**
  (`src/mcp.rs`) must be updated to (a) write canonical state as truths and (b) describe
  the new output shape and `add_truth` tool. This is in-scope follow-up documentation, not
  a code dependency, and is tracked as a task in the implementation plan.

## Part 3 — Skills subsystem

### What a skill is

A markdown file with frontmatter (`name`, `description`, body). A source repo holds many.
`github.com/jasonswett/llm-skills` is the canonical example.

### Storage

Skill **metadata** → truths on a `type:"skill"` entity. Skill **body** → a dedicated blob
table (axis B — out of FTS, single-row reinstall):

```sql
CREATE TABLE IF NOT EXISTS mcp_skills (
    entity_name  TEXT PRIMARY KEY,
    body         TEXT NOT NULL,
    source       TEXT NOT NULL,   -- repo URL
    version      TEXT NOT NULL,   -- clone commit SHA
    installed_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
);
```

Per installed skill:

- Entity `skill:<source-slug>:<name>`, type `skill`.
  - `<source-slug>` = `<owner>-<repo>` derived from the repo URL (e.g.
    `jasonswett-llm-skills`).
  - `<name>` = frontmatter `name` (fallback: file stem).
  - Namespaced name ⇒ no cross-source collisions; agents fetch the fully-qualified name.
- Truths: `description`, `source`, `version`, `installed`.
- `mcp_skills` row: full body + source + commit SHA.

### Commands

`asobi skills <subcommand>`:

- **`skills`** (no subcommand) — list installed skills, grouped by source:
  `skill:<slug>:<name> · <description> · <version>`. Lazy (truths only, no bodies).
- **`skills install <source>`**
  1. `git clone --depth 1 <source>` into a tempdir.
  2. Walk for `*.md`; parse frontmatter (`name`, `description`). Files without parseable
     frontmatter are skipped with a warning.
  3. **Select**: interactive multiselect when stdout is a TTY; `--all` or
     `--select <name>...` for non-interactive callers (agents/scripts). Error if neither a
     TTY nor a selection flag is given.
  4. Resolve the clone's `HEAD` commit SHA (`git rev-parse HEAD`).
  5. For each chosen skill: upsert entity + truths + `mcp_skills` row, in one transaction.
  6. Delete the tempdir.
- **`skills update [source]`** — re-clone (all installed sources, or the named one),
  refresh `mcp_skills.body`, and bump the `version` truth + row to the new SHA. All-in-DB;
  no filesystem residue, so agents need no new read permissions.
- **`skills remove <name|source>`** — delete the entity (body cascades) by fully-qualified
  name, or every skill under a source slug.

### Agent fetch path

No new fetch command — agents use existing `open-nodes`:

```
asobi open-nodes "skill:jasonswett-llm-skills:writing-tests"
```

returns the entity's truths + the body (eager). Discovery is `asobi skills` or
`search-nodes`.

### Build / dependencies

- Skills core lives in the **base build** (no `documents` feature): graph + `mcp_skills`
  table + git.
- `git` is shelled out (`git clone --depth 1`, `git rev-parse HEAD`) — Nix-first, already
  present; avoids the `git2`/libgit2 build weight. No HTTP client (we clone, not raw-fetch).
- Move `walkdir` from the `documents` feature to a base dependency for the repo walk.
- **When `documents` is enabled** (additive, feature-gated): also chunk + embed skill
  bodies into the existing vector store so `asobi query` finds skills semantically.

### New module

`src/skills.rs` — clone, walk, frontmatter parse, install/update/remove/list orchestration.
DB helpers (`skill_upsert`, `skill_body`, `list_skills`, plus `truth_upsert` /
`truth_delete` / truth selects) live in `src/db.rs` beside the other `mcp_*` functions; SQL
constants in `src/constant.rs`.

## Out of scope (YAGNI)

- `--full` flag on `read-graph` / `search-nodes` — truths make lazy bounded; `open-nodes`
  and `export` already cover full dumps.
- Truth FTS / `search-truths` — add only if a need appears.
- Raw-URL (non-git) skill install — clone covers the stated use case.
- A separate `skills show` command — `open-nodes` is the fetch path.

## Open questions

None blocking. Decided during design:

- Truth semantics: **keyed upsert** (bounded, canonical) over plain append.
- Skill body: **dedicated table** over body-as-observation (keeps FTS clean, one-row
  reinstall).
- Naming: **`skill:<source-slug>:<name>`** (namespaced, collision-free).
- Fetch/clone: **shell `git`** over the `git2` crate.
- Observation bound: **hard per-entity cap, evict oldest, default 50, configurable** (not
  a read-time cap) — truths make the evicted tail disposable.

## Test plan (high level)

- `add-truth` upsert: setting the same key twice keeps one row, latest value.
- Lazy reads: `read-graph` / `search-nodes` return truths + count, never observation
  content or body; `open-nodes` returns all.
- Observation cap: inserting past the limit keeps exactly the newest N (oldest evicted);
  `observation_count` reflects the capped total; `0` disables the cap.
- `delete-entities` cascades truths and skill bodies.
- Skills install: frontmatter parsing (valid / missing / malformed), namespaced naming,
  `--all` vs `--select` vs TTY-required error, tempdir cleanup, commit SHA recorded.
- Skills update: body refreshed, version bumped, no orphan rows.
- Skills remove: by name and by source slug.
- `documents` feature: skill bodies are queryable via `asobi query`.
