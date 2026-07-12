# Asobi: Usage Guide

## For humans

### Installation

From source via cargo (Rust 1.85+ toolchain required for edition 2024):

```bash
cargo install --git https://github.com/azusachino/asobi asobi
```

Prebuilt binary via `cargo-binstall` (once GitHub releases are published):

```bash
cargo binstall asobi
```

Or build locally:

```bash
git clone https://github.com/azusachino/asobi && cd asobi
make build            # graph CLI at ./target/debug/asobi
make build-documents  # includes ingest/query/compact
```

### Workspace setup

Run once on a new machine — defaults to user-level XDG paths:

```bash
asobi init
# created  ~/.local/share/asobi/data
# created  ~/.local/share/asobi/topics
# created  ~/.local/share/asobi/config
```

The user-level workspace is a single `$XDG_DATA_HOME/asobi/` root (default `~/.local/share/asobi/`) holding the same `{data,config,topics,caches}` subtree as a project-local `.asobi/`. `XDG_DATA_HOME` is honored on every platform — macOS included. No root or elevation needed: it lives inside `$HOME` and is owned by the invoking user.

To keep a project's graph isolated and checked in alongside the code, use the local layout:

```bash
cd ~/code/my-project
asobi init --local
# writes ./asobi.toml + ./.asobi/{data,topics,config}/
```

`asobi.toml` (project-local mode):

```toml
data_dir   = ".asobi/data"
config_dir = ".asobi/config"
topics_dir = ".asobi/topics"
```

Path resolution order at runtime: project-local `asobi.toml` → project-local `.asobi/` → XDG. Both `init` modes are idempotent.

Add `.asobi/` to `.gitignore`; the `asobi.toml` itself can be checked in.

### Common workflows

**Start a work session — load prior context:**

```bash
asobi search --where status=IN_PROGRESS
asobi show "my-project:session"
```
**Store a decision (supports hierarchical naming and seeded observations):**

```bash
asobi new "project-x:architecture" "project" --obs "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts (preserves case and dots):**

```bash
asobi new "UserPreferences" "preference"
asobi new "CLAUDE.md" "reference"
asobi link "project-x" "UserPreferences" "follows"
```

**Search (supports Turso FTS, segment matching, and truth filters):**

```bash
asobi search "tokio"           # finds "tokio" and "tokio-util"
asobi search "mobile"          # finds "ame:mobile-support:task-1" (segment match)
asobi search "auth*"           # prefix: matches "auth", "authentication", "authorize"
asobi search "async AND error" # both words must appear
asobi search "deploy OR ship"  # either word
asobi search "auth" --limit 25 # override the default top 100 matches
asobi search --where status=READY # find all entities with status truth set to READY
asobi search "bug" --where status=READY --where priority=high # filter by multiple truths AND the query
```

Use `graph` for full export. `search` is intentionally top-K by default so a broad term does not accidentally return the whole graph.

**End a session — persist state:**

```bash
asobi truth "my-project:session" "status" "DONE"
asobi truth "my-project:session" "last-updated" "2026-05-21"
asobi obs "my-project:session" "next: implement FTS5 index"
asobi compact  # refreshes the recall index; session state already lives in the graph
```

`compact` syncs only durable *knowledge* entities (project, decisions, references,
preferences) to Markdown + the FTS/vector index. Volatile state (`session`, `task`)
and self-indexing `skill` entities stay graph-only — query them with `search` / `show`,
and use `export` / `backup` for full archival.

**Inspect the full graph:**

```bash
asobi stats                                # Quick count of entities, relations, observations
asobi graph | jq '.data.entities[] | select(.entityType == "session")'
```

### Backup, restore, and portable export

| Goal | Command | Includes |
| --- | --- | --- |
| Portable handoff | `asobi export -o graph.json` | Entities, observations, truths, relations |
| Scoped handoff | `asobi export --scope "proj:epic" -o epic.json` | One epic subtree |
| Full libSQL archive | `asobi backup` | Complete database, including skills and documents |

```bash
asobi import graph.json
asobi backup                       # backups/asobi-<timestamp>.db; keep newest 3
asobi backup --keep 5
asobi backup -o /secure/asobi.db   # explicit path; never overwrites
asobi restore /secure/asobi.db     # validate, save current DB, then prompt
asobi restore /secure/asobi.db --force
```

- `--keep` applies only to managed snapshots, not an explicit `-o` path.
- Snapshots are integrity-checked and owner-only on Unix.
- Restore writes `backups/pre-restore-*.db`, closes live handles, atomically replaces the database, and removes stale WAL sidecars.
- Turso does not support physical backup/restore; use JSON export/import instead.

Scoped export is designed for handing an epic to another agent:

- Includes each root, transitive `part_of` children, and one-hop `depends_on` targets.
- `--rationale` adds one hop of `supersedes`/`extends` from cited decisions.
- Excludes `session`, `preference`, and `standard` entities.
- Produces ordinary JSON consumed by `asobi import`.

```bash
asobi export --scope "proj:epic" --scope "proj:other-epic" -o bundle.json
asobi export --scope "proj:epic" --rationale -o bundle.json
```

**Manage truths (structured key-value attributes):**

```bash
asobi truth "project-x" "language" "rust"
asobi rm-truth "project-x" "language"
asobi history "project-x"            # all superseded truth values, newest first
asobi history "project-x" "language" # history for one truth key
```

Overwriting a truth records the previous value with its valid-time window; the
current value stays a single row. History is opt-in via `history` (never shown in
`search`/`graph`/`show`) and is local — JSON `export`/`import` carries current
graph state only, not the change log.

**Manage skills (reusable workflows and knowledge):**

```bash
asobi skills install https://github.com/azusachino/asobi-skills --all
asobi skills
asobi skills show my-skill
asobi skills update
asobi skills remove asobi-skills
```


**Ingest Markdown into the document tier (optional):**

These commands require a binary built with `--features documents`:

```bash
asobi ingest ./notes/             # directory of .md files
asobi query "async cancellation"  # semantic + FTS search
```

---

## For agents

### Overview

Asobi is a persistent, project-local knowledge graph. Agents use it to:

- **Persist** decisions, task state, and user preferences across sessions
- **Share** context with other agents working on the same project
- **Resume** work without re-deriving context from git history or code

All operations are CLI commands. No server to start. No authentication. Latency is <10ms for graph operations.

### Session protocol

**At session start:**

```bash
# Option A: load a specific entity
asobi show "<project>:session"

# Option B: query by status truth
asobi search --where status=IN_PROGRESS

# Option C: full graph (small projects)
asobi graph
```

**During session — record facts as you learn them:**

```bash
asobi obs "<project>" "Decided to use WAL mode for concurrent agent access"
asobi truth "<project>:session" "status" "IN_PROGRESS"
```

**At session end:**

```bash
# Update volatile state
asobi truth "<project>:session" "status" "DONE"
asobi truth "<project>:session" "last-updated" "2026-05-21"
asobi obs "<project>:session" "completed: implemented FTS5 search"
asobi obs "<project>:session" "next: add WAL mode and entity_name index"

# Archive to markdown (durable, re-indexed)
asobi compact
```

**Full session reset (next agent starts clean):**

```bash
asobi rm "<project>:session"
# Next agent creates it fresh and sets initial status
asobi new "<project>:session" "session"
asobi truth "<project>:session" "status" "IN_PROGRESS"
```

### Entity naming conventions

| Pattern             | Type         | Purpose                                      |
| ------------------- | ------------ | -------------------------------------------- |
| `<project>:session` | `session`    | Volatile task state — reset each session     |
| `<project>:tasks`   | `task`       | In-progress task tracking                    |
| `<project>`         | `project`    | Stable project facts, architecture decisions |
| `UserPreferences`   | `preference` | Cross-project user habits                    |
| `CodingStyle`       | `standard`   | Commit format, indentation, etc.             |
| `ToolPreferences`   | `preference` | Nix, make, etc.                               |

### Machine-readable response contract

Graph reads and commands invoked with `--json` return a versioned envelope:

```json
{
  "schemaVersion": 1,
  "ok": true,
  "data": { "entities": [], "relations": [] }
}
```

On failure with `--json`, `ok` is `false` and `error` is present instead of
`data`:

```json
{
  "schemaVersion": 1,
  "ok": false,
  "error": { "kind": "not_found", "message": "not found: missing" }
}
```

Use `asobi schema` to discover the envelope and command-specific JSON Schemas;
use `asobi schema --command show` for one command. Consumers should read graph
fields through `.data` and branch on `.error.kind`. The response schema version
is independent from the storage/export `apiVersion`.

### Output format

Asobi operates under a **lazy-read contract** to minimize token overhead.

The `data` payload for `graph` and `search` is a lazy JSON structure (excluding
`observations` and skill `body`, only providing `truths` and
`observationCount`). The command output wraps this payload in the envelope
described above:

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
      "body": "string"
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

Mutation commands print a one-line confirmation: `Entity 'X' created.`, `Observation added.`, etc.

### Multi-agent context handoff

When Agent A finishes and Agent B picks up:

```bash
# Agent A (end of session)
asobi obs "project-x:session" "status: BLOCKED"
asobi obs "project-x:session" "next: Agent B should implement WAL mode in src/db.rs init_db()"
asobi compact

# Agent B (start of session)
asobi show "project-x:session"
# → reads: status BLOCKED, next action, last-updated
```

No files to pass, no state to reconstruct. The graph is the handoff.

### Search tips

`search` uses Turso's native full-text index. Queries match indexed terms; there is no
SQLite FTS5 porter stemming. Practical implications:

- `search "run"` → matches the indexed term "run" (use the exact term when needed)
- `search "implement"` → matches the indexed term "implement"
- `search "tokio async"` → finds entities with both words (ranked higher) or either word
- `search "UserPreferences"` → exact name match via LIKE fallback (entity has no observations)
- `search "AND AND"` → invalid full-text syntax, silently falls back to LIKE, returns empty
- `search "auth" --limit 500` → return more than the default top 100 matches
- `search --where KEY=VALUE` → filters matching entities by truth values (e.g. `--where status=READY`). Can be repeated; multiple filters perform an intersection (AND condition). If a query term is also provided, it matches the intersection of the filters and the FTS/LIKE results.

For exact entity retrieval, prefer `show` over `search`:

```bash
asobi show "project-x:session" "UserPreferences"
```

`search` accepts an optional `--limit` argument. Omit it for the default top 100 matches; set it explicitly for larger ranked exports.

## Running in Sandboxed Environments (Codex, etc.)

When running in sandboxed or highly restricted environments (such as Codex, Nix build sandboxes, or certain containerized runners), the environment might impose constraints on directory write access or shared-memory creation. Asobi can be configured to run smoothly in these environments using the following techniques:

### Project-Local Workspace
Use the project-local setup to avoid writing to the global `~/.local` (XDG) directory, which may be read-only or non-existent:
```bash
asobi init --local
```
This writes an `asobi.toml` file in the current working directory and places database and configurations within the `./.asobi/` subdirectory.

### Custom Database Paths
You can override Asobi's home or database locations using environment variables:
- **`ASOBI_HOME`**: Changes the base directory under which Asobi looks for configuration, data, and topics (e.g. `ASOBI_HOME=/tmp/asobi`).
- **`ASOBI_DATABASE_URL`**: Specifies the direct path to the database file itself (e.g. `ASOBI_DATABASE_URL=/tmp/asobi-custom.db`).

### Turso concurrency
Turso uses experimental multi-process WAL with bounded retries for startup and
immediate write transactions. Legacy `ASOBI_BUSY_TIMEOUT` and
`ASOBI_JOURNAL_MODE` overrides are not supported.
