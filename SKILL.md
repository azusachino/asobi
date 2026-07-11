---
name: asobi
description: Use Asobi CLI to store and retrieve long-term project memory (Entities, Observations, Relations) via a persistent Knowledge Graph backed by libSQL.
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
- **Observation** — append-only log line, capped (oldest evicted past the limit, default 50). The *trail*.
- **Truth** — a `key→value` fact that upserts. The *current state* (`status`, `version`).
- **Relation** — directed edge `(from, to, type)`.
- **Skill** — an installed instruction: Markdown body + `description`/`source`/`version` truths.

`graph`/`search` return truths + `observationCount` only (cheap); `show` also returns observations and the skill body.

---

## Command Reference

Stream contract (matters for scripted callers): **mutating** commands print a one-line confirmation (`Entity 'X' created.`, `Observation added.`, etc.) to **stderr** and leave **stdout empty** on success — check the exit code, not stdout. **Read** commands (`graph`, `search`, `show`, `stats`, `export`) write their result to **stdout**; graph reads emit JSON.

Pass the global `--json` flag to any mutating command to also print the affected entity/entities (and the relations among them) as JSON to **stdout** — e.g. `asobi new A task --json`. This removes the follow-up `show` round-trip; `rm --json` instead prints `{"deleted": [...]}`.

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

Returns the full graph as JSON: `{ "entities": [...], "relations": [...] }`. Each entity includes all its observations.

```
asobi search <QUERY> [--limit <N>] [--where KEY=VALUE ...]
```

Returns a subgraph (same JSON shape) of entities matching `QUERY`. Uses two search paths, merged in order:

1. **FTS5 on observations** — porter stemming + BM25 ranking. `"run"` matches `"running"`, `"tokio async"` ranks entities that contain both words higher. Supports FTS5 operators: `AND`, `OR`, `NOT`, prefix with `*` (e.g. `auth*`).
2. **LIKE on entity name / type** — substring fallback, always runs. Catches exact-name lookups (`UserPreferences`) and entities with no observations.

Supports filtering the search results by matching entity truths via repeatable `--where KEY=VALUE` filters (e.g. `asobi search --where status=READY`). If multiple `--where` filters are specified, they are treated as an intersection (AND condition).

Relations between matched entities are included. Results are ordered by BM25 relevance (FTS matches first, then name/type matches).
The default limit is 100 matched nodes; use `--limit` for larger ranked exports.
Use `graph` when the caller needs the full graph; do not use a broad
`search` query as an implicit export.

```
asobi show <NAME> [<NAME> ...] [--expand <RELATION_TYPE> ...] [--with-ids]
```

Returns a subgraph for the named entities plus relations between them. Takes one or more names as positional args.
* `--expand <RELATION_TYPE>`: repeatably expand relations of a given type. Useful for loading subtrees (e.g. `--expand part_of` to eagerly load related epic tasks).
* `--with-ids`: include `observationsDetailed` list showing exact unique integer IDs (`id`) for each observation.


### Truths

```
asobi truth <NAME> <KEY> <VALUE>
```

Add or update a truth key-value pair for the named entity.

```
asobi rm-truth <NAME> <KEY>
```

Delete a specific truth key from the named entity.

### Skills Subsystem

```
asobi skills
```

List all installed skills, grouped by source.

```
asobi skills install <SOURCE> [--all | --select <NAME>...]
```

Install skills from a local path or git repository. Pick with `--all`, `--select <NAME>...`, or neither — which prompts an interactive numbered picker (TTY required; otherwise it errors asking for a flag). Git sources are shallow-cloned to a reused cache under `.asobi/caches/<slug>`. Frontmatter gives the metadata (name falls back to the file/dir name); the body is stored, and also embedded into the vector tier when built with `documents`. `--all` is a full **sync**: skills previously installed from the source but no longer present upstream (deleted or renamed) are pruned. `--select` and the interactive picker stay purely additive.

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
* `--prefix`: deletes all observations under the entity matching the content string as a prefix, rather than requiring an exact match.


```
asobi unlink <FROM> <TO> <RELATION_TYPE>
```

Removes a single relation by its three-part key.

### Document ingestion / vector recall

Available only in binaries built with Cargo feature `documents`
(`cargo build --features documents` or `make build-documents`).

```
asobi ingest <PATH>
```

Ingests a file or directory of Markdown files into the document tier (chunks, embeds, stores in libSQL). Used for semantic search across long-form content.

```
asobi query <QUERY>
```

Hybrid semantic + FTS keyword search over ingested topics. Returns: `TITLE | (score: X.XX) | PATH` per result.

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
2. Finds near-duplicate topic clusters in the vector store (cosine ≥ 0.85).
3. Syncs **durable knowledge** entities (`project`, `concept`, `reference`, `preference`, `standard`) back to a Markdown file in `.asobi/topics/` — including their truths — and re-ingests for FTS/vector freshness. Volatile state (`session`, `task`/epic) and self-indexing `skill` entities are skipped: they stay graph-only (read them with `search` / `show`), and `export` / `backup` cover full archival.

### Physical backup and restore

```bash
asobi backup                              # default timestamped snapshot; keeps newest 3
asobi backup --keep 5                     # retention applies to the default backup directory
asobi backup -o /secure/path/asobi.db     # explicit destination; refuses to overwrite
asobi restore /secure/path/asobi.db       # validates, then asks before replacing live data
asobi restore /secure/path/asobi.db --force
```

Physical backup/restore is a full-fidelity archive of the selected libSQL database, including graph state, installed skill bodies, and document data. Snapshots are integrity-checked and owner-only on Unix. Before restore, Asobi creates a timestamped `pre-restore-*.db` safety snapshot, closes the live database handles, atomically replaces the database, and clears stale WAL sidecars. The experimental Turso backend does not support this capability. Use JSON `export`/`import` instead for portable cross-backend or teammate handoff.

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
# knowledge recall index (it skips session/task entities). Use export/backup
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

**Graph commands** return a JSON object containing `entities` and `relations`.

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

**Mutating commands** print a plain-text confirmation line to **stderr** (no JSON, stdout empty). Rely on the process exit code for success/failure, then `show` the affected entity if you need to read back the result.

---

## Storage Layout

```
.asobi/
  data/
    asobi.db        # libSQL: mcp_entities, mcp_observations, mcp_truths, mcp_relations, mcp_skills, topics, topics_fts, chunks
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
asobi obs "my-project" "Uses libSQL for storage — chosen for embedded + remote parity"

# Store user preference
asobi new "UserPreferences" "preference"
asobi obs "UserPreferences" "Prefer make over cargo commands directly"

# Link them
asobi link "my-project" "UserPreferences" "follows"

# Resume context
asobi show "my-project" "UserPreferences"

# Correct a stale observation
asobi update-obs "my-project" "Uses libSQL for storage — chosen for embedded + remote parity" "Uses libSQL (libsql crate v0.6) — embedded SQLite with Turso remote sync option"

# Search by keyword
asobi search "libSQL"

# Deliberately request a larger ranked result set
asobi search "auth" --limit 500
```
