---
name: asobi
description: Use Asobi CLI to store and retrieve long-term project memory (Entities, Observations, Relations) via a persistent SQLite Knowledge Graph.
---

# Asobi Memory Skill

Asobi maintains a persistent Knowledge Graph of project facts, user preferences, and technical decisions. Data is stored locally in `.asobi/data/asobi.db` (project-local when `asobi.toml` or `.asobi/` exists in the working directory).

## When to use

- **New fact learned**: stable architecture decision, tool preference, team convention.
- **Session start**: load prior context for the current project.
- **Session end**: persist status, completed work, and next steps.
- **Context handoff**: another agent or session needs to resume where you left off.

---

## Concepts

One graph, a few node parts — knowing which to write is the skill:

- **Entity** — a named node with a `type`.
- **Observation** — append-only log line, capped (oldest evicted past the limit, default 200). The _trail_.
- **Truth** — a `key→value` fact that upserts. The _current state_ (`status`, `version`).
- **Relation** — directed edge `(from, to, type)`.
- **Skill** — an installed instruction: Markdown body + `description`/`source`/`version` truths.

`graph`/`search` return a lean entity index with truths, `observationCount`, and relations only (cheap); `show` also returns observations and the skill body. Use `export` or `backup` when a complete archival payload is required.

---

## Command Reference

Stream contract (matters for scripted callers): **mutating** commands print a one-line confirmation (`Entity 'X' created.`, `Observation added.`, etc.) to **stderr** and leave **stdout empty** on success — check the exit code, not stdout. **Read** commands (`graph`, `search`, `show`, `stats`, `export`) write their command-specific JSON result to **stdout**. Use `asobi schema --command NAME` to discover the payload contract.

Pass the global `--json` flag to any mutating command to also print the affected entity/entities (and the relations among them) as JSON to **stdout** — e.g. `asobi new A task --json`. This removes the follow-up `show` round-trip; `rm --json` returns `{ "deleted": [...] }`.

### Create

```
asobi new <NAME> <ENTITY_TYPE> [<NAME> <ENTITY_TYPE> ...] [--obs <OBSERVATION> ...]
```

Creates one or more entities in a single call — pass repeated `NAME TYPE` pairs (`new A task B concept` creates two). The argument count must be a multiple of 2. Silently no-ops on names that already exist (`INSERT OR IGNORE`). Prefer one batched call over many invocations.

Supports seeding observations at creation via repeatable `--obs <OBSERVATION>` options (e.g. `asobi new my-node task --obs "initial observation"`). If multiple entities are created in a single call, the specified observations are seeded to all of them.

```
asobi obs <NAME> <CONTENT> [<CONTENT> ...]
```

Appends one or more observation strings to an existing entity. The entity must already exist. Observations are subject to a rolling history cap (defaults to 200, oldest evicted per entity, customizable via `ASOBI_OBSERVATION_LIMIT` or `asobi.toml`'s `observation_limit`).

```
asobi link <FROM> <TO> <RELATION_TYPE> [<FROM> <TO> <RELATION_TYPE> ...]
```

Creates one or more directed relations in a single call — pass repeated `FROM TO TYPE` triples (`link A B uses C D blocks`). The argument count must be a multiple of 3. Upserts on the composite key `(from, to, relation_type)`.

### Read

```
asobi graph
```

Returns the full lean graph as `{ "entities": [...], "relations": [...] }`; entities include truths and observation counts, while observation bodies remain lazy. Use `show` for selected observations and `asobi schema --command graph` for the payload contract.

```
asobi search <QUERY> [--limit <N>] [--where KEY=VALUE ...]
```

Returns a subgraph (same payload shape) of entities matching `QUERY`. Use `asobi schema --command search` for the payload contract. Uses two search paths, merged in order:

1. **FTS5 on observations** — porter stemming + BM25 ranking. `"run"` matches `"running"`, `"tokio async"` ranks entities that contain both words higher. Supports FTS5 operators: `AND`, `OR`, `NOT`, prefix with `*` (e.g. `auth*`).
2. **LIKE on entity name / type** — substring fallback, always runs. Catches exact-name lookups (`UserPreferences`) and entities with no observations.

Supports filtering the search results by matching entity truths via repeatable `--where KEY=VALUE` filters (e.g. `asobi search --where status=READY`). If multiple `--where` filters are specified, they are treated as an intersection (AND condition).

Relations between matched entities are included. Results are ordered by BM25 relevance (FTS matches first, then name/type matches). The default limit is 100 matched nodes; use `--limit` for larger ranked exports. Use `graph` when the caller needs the full graph; do not use a broad `search` query as an implicit export.

```
asobi show <NAME> [<NAME> ...] [--expand <RELATION_TYPE> ...] [--with-ids]
```

Returns a subgraph for the named entities plus relations between them. Takes one or more names as positional args.

- `--expand <RELATION_TYPE>`: repeatably expand relations of a given type. Useful for loading subtrees (e.g. `--expand part_of` to eagerly load related epic tasks).
- `--with-ids`: include `observationsDetailed` list showing exact unique integer IDs (`id`) for each observation.

Do not use `graph` or broad `search` as a document export. Fetch heavy content only for the specific entities needed with `show`.

### Truths

```
asobi truth <NAME> <KEY> <VALUE>
```

Add or update a truth key-value pair for the named entity. Overwriting a truth records the superseded value in an append-only history with its valid-time window, so the current state stays a single value while the change trail is preserved.

```
asobi rm-truth <NAME> <KEY>
```

Delete a specific truth key from the named entity.

```
asobi history <NAME> [KEY]
```

Show an entity's truth change history — each superseded value with the `validFrom`/`validUntil` interval it was current for, newest first. Pass a `KEY` to narrow to a single truth. The value that is current now lives on the entity (`show`), not in history. History is recorded automatically on every overwrite and never appears in `search`/`graph`/`show`, so the default reads stay unchanged. It is local physical state and is **not** carried by JSON `export`/`import`.

### Skills Subsystem

```
asobi skills
```

List all installed skills, grouped by source.

```
asobi skills install <SOURCE> [--all | --select <NAME>...]
```

Install skills from a local path or git repository. Pick with `--all`, `--select <NAME>...`, or neither — which prompts an interactive numbered picker (TTY required; otherwise it errors asking for a flag). Git sources are shallow-cloned to a reused cache under `.asobi/caches/<slug>`. Frontmatter gives the metadata (name falls back to the file/dir name); the body is stored as graph-backed skill data. `--all` is a full **sync**: skills previously installed from the source but no longer present upstream (deleted or renamed) are pruned. `--select` and the interactive picker stay purely additive.

```
asobi skills update [SOURCE]
```

Refreshes installed skills (or one source) from the cache via `git fetch` + `reset --hard`, re-cloning if that fails. Like `install --all`, it syncs — skills dropped upstream are pruned. Needs `git` on `PATH`; unreachable remotes fail with a clear error.

```
asobi skills remove <NAME | SOURCE>
```

Remove a specific skill by its name or all skills from a source URL/slug.

```
asobi skills show <NAME>
```

Show the raw body of an installed skill without JSON escaping. Useful for humans to read. <NAME> can be fully-qualified (e.g. `skill:slug:name`) or just the short name.

### Delete

```
asobi rm <NAME> [<NAME> ...]
```

Deletes one or more entities and all their observations and relations (cascades).

```
asobi update-obs <NAME> <OLD_CONTENT> <NEW_CONTENT>
```

Atomically replaces an existing observation `<OLD_CONTENT>` under the entity with `<NEW_CONTENT>`.

```
asobi rm-obs <NAME> <CONTENT> [--prefix]
```

Removes matching observations from the named entity.

- `--prefix`: deletes all observations under the entity matching the content string as a prefix, rather than requiring an exact match.

```
asobi unlink <FROM> <TO> <RELATION_TYPE>
```

Removes a single relation by its three-part key.

### Workspace init

```
asobi init           # XDG (default) — user-level dirs under $HOME
asobi init --local   # project-local — ./.asobi/ + ./asobi.toml
```

Idempotent in both modes. Run `asobi init` once on a new machine; run `asobi init --local` inside a project root when you want an isolated, project-scoped graph.

### Maintenance

```
asobi compact [--older-than <DAYS>]
```

Three-step maintenance sweep:

1. Prunes session Markdown files in `.asobi/topics/sessions/` older than `DAYS` (default: 90).
2. Syncs **durable knowledge** entities (`project`, `concept`, `reference`, `preference`, `standard`) back to a Markdown file in `.asobi/topics/` — including their truths. Volatile state (`session`, `task`/epic) and self-indexing `skill` entities are skipped: they stay graph-only (read them with `search` / `show`), and `export` / `backup` cover full archival.

```bash
asobi purge [--dry-run]
asobi purge --type task --status DONE --older-than 90 --apply
```

`purge` is dry-run by default and accepts only `session` plus terminal `task` statuses (`DONE`, `CLOSED`, or `ABANDONED`). It never accepts durable knowledge or skills. Review the candidate report before adding `--apply`; it does not run implicitly during `graph`, `search`, `compact`, or startup.

### Shell completion

```bash
asobi completions bash|elvish|fish|powershell|zsh
```

Generate completion scripts from the installed binary so command and flag names stay aligned with the running Asobi version.

### Physical backup and restore

```bash
asobi backup                          # timestamped snapshot; keep newest 3
asobi backup --keep 5                 # retention for managed snapshots
asobi backup -o /secure/asobi.db      # explicit path; never overwrites
asobi restore /secure/asobi.db        # validate, save current DB, then prompt
asobi restore /secure/asobi.db --force
```

- **Includes:** graph state and skill bodies.
- **Safety:** integrity check, owner-only snapshot, `pre-restore-*.db`, closed handles, atomic replacement, stale sidecar cleanup.
- **Scope:** the local SQLite database. Use JSON `export`/`import` for teammate or machine handoff.
- **Retention:** `--keep` applies only to managed snapshots under `backups/`, not an explicit `-o` path.

### Performance verification

```bash
ASOBI_BENCH_SIZES=1000,10000 make bench-graph
ASOBI_BENCH_SIZE=10000 make bench-criterion
ASOBI_BENCH_SIZE=10000 make bench-alloc
make bench-sql-plans
make bench-tasks
make bench-storage
```

Compare Criterion medians and confidence intervals under `target/criterion/`. Open `dhat-heap.json` with DHAT's viewer to find allocation-heavy call stacks. SQL plans should seek `idx_asobi_truths_lookup` for truth filters and use both relation indexes for neighborhood lookup. Use the same dataset size, commit profile, and machine for before/after comparisons.

---

## Entity Type Conventions

Use consistent types so `search` and `show` filters are predictable:

| Type         | Use for                                          |
| ------------ | ------------------------------------------------ |
| `project`    | Per-project stable facts, architecture decisions |
| `session`    | Volatile task state — reset each session end     |
| `preference` | Cross-project user or tool preferences           |
| `standard`   | Global conventions that apply everywhere         |
| `concept`    | Technical concepts, definitions                  |
| `task`       | In-progress task lists, status tracking          |
| `reference`  | Pointers to external resources, URLs             |

---

## Session Protocol

### Session Start

```bash
asobi search --where status=IN_PROGRESS      # find active session entities
asobi show "<project>:session"              # load specific session state
```

Or load everything and filter client-side:

```bash
asobi graph
```

### Session End

```bash
# Update session state
asobi truth "<project>:session" "status" "DONE"
asobi truth "<project>:session" "last-updated" "YYYY-MM-DD"
asobi obs "<project>:session" "next: <one sentence handoff>"

# Session state already lives in the graph; compact only refreshes the
# Markdown topic projection (it skips session/task entities). Use export/backup
# for full archival.
asobi compact
```

### Full Session Reset (next agent starts clean)

```bash
asobi rm "<project>:session"
# recreate at next session start
asobi new "<project>:session" "session"
asobi truth "<project>:session" "status" "IN_PROGRESS"
```

---

## Naming Conventions

- Session entities: `<project-name>:session` (e.g. `asobi:session`)
- Epics / tasks: `<project>:<epic>` and `<project>:<epic>:task-<n>` (e.g. `asobi:skills-truths:task-1`), linked `part_of` the epic
- Skills: `skill:<source-slug>:<name>` (e.g. `skill:jasonswett-llm-skills:tdd`)
- Cross-project preferences: `UserPreferences`, `CodingStyle`, `ToolPreferences`
- Relations use verb phrases: `uses`, `depends-on`, `validates`, `extends`, `blocks`

---

## Output Format Reference

**Graph commands** return their documented JSON payload directly. Use `asobi schema --command graph` to discover the exact shape.

`graph` and `search` use a **lazy-read contract** (they do not populate observation content or skill bodies, returning only `observationCount` and `truths`):

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "truths": {
        "key": "value"
      },
      "observationCount": 12
    }
  ],
  "relations": [
    {
      "from": "string",
      "to": "string",
      "relationType": "string"
    }
  ]
}
```

`show` eagerly returns all `observations` and the skill `body` (if it's a skill entity):

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "observations": ["string", ...],
      "truths": {
        "key": "value"
      },
      "observationCount": 12,
      "body": "string",
      "observationsDetailed": [
        {
          "id": 123,
          "content": "string"
        }
      ]
    }
  ],
  "relations": [
    {
      "from": "string",
      "to": "string",
      "relationType": "string"
    }
  ]
}
```

**Mutating commands** print a plain-text confirmation line to **stderr** (no JSON, stdout empty) unless global `--json` is passed. With `--json`, read the affected payload directly.

---

## Storage Layout

```
.asobi/
  data/
    asobi.db        # SQLite: entities, observations, truths, relations, skills, and FTS5 index
  caches/              # Persistent shallow clones of git skill sources (skills install/update)
    <slug>/
  topics/              # Markdown snapshots synced by `compact`
    <slug>.md
    sessions/          # Session files pruned by compact --older-than
```

The user-level (XDG) workspace mirrors this exact tree under a single root, `$XDG_DATA_HOME/asobi/` (default `~/.local/share/asobi/`), honoring `XDG_DATA_HOME` on every platform — macOS included.

Controlled by `asobi.toml` in the project root (takes precedence over XDG paths). Generated by `asobi init`:

```toml
data_dir   = ".asobi/data"
config_dir = ".asobi/config"
topics_dir = ".asobi/topics"
```

---

## Quick Examples

```bash
# Store a project decision
asobi new "my-project" "project"
asobi obs "my-project" "Uses SQLite WAL for local multi-agent concurrency"

# Store user preference
asobi new "UserPreferences" "preference"
asobi obs "UserPreferences" "Prefer make over cargo commands directly"

# Link them
asobi link "my-project" "UserPreferences" "follows"

# Resume context
asobi show "my-project" "UserPreferences"

# Correct a stale observation
asobi update-obs "my-project" "Uses SQLite WAL for local multi-agent concurrency" "Uses SQLite WAL with bounded busy timeouts for local multi-agent concurrency"

# Search by keyword
asobi search "SQLite"

# Deliberately request a larger ranked result set
asobi search "auth" --limit 500
```
