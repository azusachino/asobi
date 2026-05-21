---
name: rosemary
description: Use Rosemary CLI to store and retrieve long-term project memory (Entities, Observations, Relations) via a persistent Knowledge Graph backed by libSQL.
---

# Rosemary Memory Skill

Rosemary maintains a persistent Knowledge Graph of project facts, user preferences, and technical decisions. Data is stored locally in `.rosemary/data/rosemary.db` (project-local when `rosemary.toml` or `.rosemary/` exists in the working directory).

## When to use

- **New fact learned**: stable architecture decision, tool preference, team convention.
- **Session start**: load prior context for the current project.
- **Session end**: persist status, completed work, and next steps.
- **Context handoff**: another agent or session needs to resume where you left off.

---

## Command Reference

All commands print a one-line confirmation (`Entity 'X' created.`, `Observation added.`, etc.) on success. Graph-read commands print JSON to stdout.

### Create

```
rosemary create-entities <NAME> <ENTITY_TYPE>
```

Creates a single entity. Silently no-ops if the name already exists (`INSERT OR IGNORE`).

```
rosemary add-observations <NAME> <CONTENT>
```

Appends one observation string to an existing entity. The entity must already exist.

```
rosemary create-relations <FROM> <TO> <RELATION_TYPE>
```

Creates a directed relation between two existing entities. Upserts on the composite key `(from, to, relation_type)`.

### Read

```
rosemary read-graph
```

Returns the full graph as JSON: `{ "entities": [...], "relations": [...] }`. Each entity includes all its observations.

```
rosemary search-nodes <QUERY> [--limit <N>]
```

Returns a subgraph (same JSON shape) of entities matching `QUERY`. Uses two search paths, merged in order:

1. **FTS5 on observations** — porter stemming + BM25 ranking. `"run"` matches `"running"`, `"tokio async"` ranks entities that contain both words higher. Supports FTS5 operators: `AND`, `OR`, `NOT`, prefix with `*` (e.g. `auth*`).
2. **LIKE on entity name / type** — substring fallback, always runs. Catches exact-name lookups (`UserPreferences`) and entities with no observations.

Relations between matched entities are included. Results are ordered by BM25 relevance (FTS matches first, then name/type matches).
The default limit is 100 matched nodes; use `--limit` for larger ranked exports.
Use `read-graph` when the caller needs the full graph; do not use a broad
`search-nodes` query as an implicit export.

```
rosemary open-nodes <NAME> [<NAME> ...]
```

Returns a subgraph for the named entities plus relations between them. Takes one or more names as positional args.

### Delete

```
rosemary delete-entities <NAME> [<NAME> ...]
```

Deletes one or more entities and all their observations and relations (cascades).

```
rosemary delete-observations <NAME> <CONTENT>
```

Removes a single observation (exact content match) from the named entity.

```
rosemary delete-relations <FROM> <TO> <RELATION_TYPE>
```

Removes a single relation by its three-part key.

### Document ingestion / vector recall

Available only in binaries built with Cargo feature `documents`
(`cargo build --features documents` or `make build-documents`).

```
rosemary ingest <PATH>
```

Ingests a file or directory of Markdown files into the document tier (chunks, embeds, stores in LanceDB). Used for semantic search across long-form content.

```
rosemary query <QUERY>
```

Hybrid semantic + FTS keyword search over ingested topics. Returns: `TITLE | (score: X.XX) | PATH` per result.

### Workspace init

```
rosemary init           # XDG (default) — user-level dirs under $HOME
rosemary init --local   # project-local — ./.rosemary/ + ./rosemary.toml
```

Idempotent in both modes. Run `rosemary init` once on a new machine; run `rosemary init --local` inside a project root when you want an isolated, project-scoped graph.

### Maintenance

```
rosemary compact [--older-than <DAYS>]
```

Three-step maintenance sweep:

1. Prunes session Markdown files in `.rosemary/topics/sessions/` older than `DAYS` (default: 90).
2. Finds near-duplicate topic clusters in the vector store (cosine ≥ 0.85).
3. Syncs every graph entity back to a Markdown file in `.rosemary/topics/` and re-ingests for FTS/vector freshness.

---

## Entity Type Conventions

Use consistent types so `search-nodes` and `open-nodes` filters are predictable:

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
rosemary search-nodes "session"       # find active session entities
rosemary open-nodes "<project>:session"  # load specific session state
```

Or load everything and filter client-side:

```bash
rosemary read-graph
```

### Session End

```bash
# Update session state
rosemary delete-observations "<project>:session" "<old status line>"
rosemary add-observations "<project>:session" "status: DONE"
rosemary add-observations "<project>:session" "next: <one sentence handoff>"
rosemary add-observations "<project>:session" "last-updated: YYYY-MM-DD"

# Archive to Markdown (durable backup + refreshes vector/FTS)
rosemary compact
```

### Full Session Reset (next agent starts clean)

```bash
rosemary delete-entities "<project>:session"
# recreate at next session start
rosemary create-entities "<project>:session" "session"
```

---

## Naming Conventions

- Session entities: `<project-name>:session` (e.g. `rosemary:session`)
- Task lists: `<project-name>:tasks`
- Cross-project preferences: `UserPreferences`, `CodingStyle`, `ToolPreferences`
- Relations use verb phrases: `uses`, `depends-on`, `validates`, `extends`, `blocks`

---

## Output Format Reference

**Graph commands** (`read-graph`, `search-nodes`, `open-nodes`) return:

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "observations": ["string", ...]
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

**Mutating commands** print a plain-text confirmation line — no JSON.

---

## Storage Layout

```
.rosemary/
  data/
    rosemary.db        # libSQL: mcp_entities, mcp_observations, mcp_relations, topics, topics_fts
    lancedb/           # LanceDB: vector chunks
  topics/              # Markdown snapshots synced by `compact`
    <slug>.md
    sessions/          # Session files pruned by compact --older-than
```

Controlled by `rosemary.toml` in the project root (takes precedence over XDG paths). Generated by `rosemary init`:

```toml
data_dir   = ".rosemary/data"
config_dir = ".rosemary/config"
topics_dir = ".rosemary/topics"
```

---

## Quick Examples

```bash
# Store a project decision
rosemary create-entities "my-project" "project"
rosemary add-observations "my-project" "Uses libSQL for storage — chosen for embedded + remote parity"

# Store user preference
rosemary create-entities "UserPreferences" "preference"
rosemary add-observations "UserPreferences" "Prefer make over cargo commands directly"

# Link them
rosemary create-relations "my-project" "UserPreferences" "follows"

# Resume context
rosemary open-nodes "my-project" "UserPreferences"

# Correct a stale observation
rosemary delete-observations "my-project" "Uses libSQL for storage — chosen for embedded + remote parity"
rosemary add-observations "my-project" "Uses libSQL (libsql crate v0.6) — embedded SQLite with Turso remote sync option"

# Search by keyword
rosemary search-nodes "libSQL"

# Deliberately request a larger ranked result set
rosemary search-nodes "auth" --limit 500
```
