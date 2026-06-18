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
asobi search "session"
asobi show "my-project:session"
```
**Store a decision (supports hierarchical naming):**

```bash
asobi new "project-x:architecture" "project"
asobi obs "project-x:architecture" "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts (preserves case and dots):**

```bash
asobi new "UserPreferences" "preference"
asobi new "CLAUDE.md" "reference"
asobi link "project-x" "UserPreferences" "follows"
```

**Search (supports FTS5, segment matching, and truth filters):**

```bash
asobi search "tokio"           # finds "tokio", "tokio-util", stemmed variants
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
asobi rm-obs "my-project:session" "status: IN_PROGRESS"
asobi obs "my-project:session" "status: DONE"
asobi obs "my-project:session" "next: implement FTS5 index"
asobi obs "my-project:session" "last-updated: 2026-05-21"
asobi compact  # syncs graph → markdown files for durable backup
```

**Inspect the full graph:**

```bash
asobi stats                                # Quick count of entities, relations, observations
asobi graph | jq '.entities[] | select(.entityType == "session")'
```

**Backup, Restore, and Reset:**

```bash
asobi export -o backup.json                # Export the entire graph to JSON
asobi import backup.json                   # Import entities and relations from a JSON backup
asobi reset                                # Interactively clear the entire graph (use --force to bypass)
```

**Manage truths (structured key-value attributes):**

```bash
asobi truth "project-x" "language" "rust"
asobi rm-truth "project-x" "language"
```

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

# Option B: keyword search
asobi search "session"

# Option C: full graph (small projects)
asobi graph
```

**During session — record facts as you learn them:**

```bash
asobi obs "<project>" "Decided to use WAL mode for concurrent agent access"
asobi obs "<project>:session" "status: IN_PROGRESS"
```

**At session end:**

```bash
# Update volatile state
asobi rm-obs "<project>:session" "<old status line>"
asobi obs "<project>:session" "status: DONE"
asobi obs "<project>:session" "completed: implemented FTS5 search"
asobi obs "<project>:session" "next: add WAL mode and entity_name index"
asobi obs "<project>:session" "last-updated: 2026-05-21"

# Archive to markdown (durable, re-indexed)
asobi compact
```

**Full session reset (next agent starts clean):**

```bash
asobi rm "<project>:session"
# Next agent creates it fresh
asobi new "<project>:session" "session"
```

### Entity naming conventions

| Pattern             | Type         | Purpose                                      |
| ------------------- | ------------ | -------------------------------------------- |
| `<project>:session` | `session`    | Volatile task state — reset each session     |
| `<project>:tasks`   | `task`       | In-progress task tracking                    |
| `<project>`         | `project`    | Stable project facts, architecture decisions |
| `UserPreferences`   | `preference` | Cross-project user habits                    |
| `CodingStyle`       | `standard`   | Commit format, indentation, etc.             |
| `ToolPreferences`   | `preference` | Nix, make, rtk, etc.                          |

### Output format

Asobi operates under a **lazy-read contract** to minimize token overhead.

`graph` and `search` return a lazy JSON structure (excluding `observations` and skill `body`, only providing `truths` and `observationCount`):

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

`search` uses FTS5 with porter stemming. Practical implications:

- `search "run"` → finds entities with "running", "runner", "ran"
- `search "implement"` → finds "implementation", "implementing"
- `search "tokio async"` → finds entities with both words (ranked higher) or either word
- `search "UserPreferences"` → exact name match via LIKE fallback (entity has no observations)
- `search "AND AND"` → invalid FTS5 syntax, silently falls back to LIKE, returns empty
- `search "auth" --limit 500` → return more than the default top 100 matches
- `search --where KEY=VALUE` → filters matching entities by truth values (e.g. `--where status=READY`). Can be repeated; multiple filters perform an intersection (AND condition). If a query term is also provided, it matches the intersection of the filters and the FTS/LIKE results.

For exact entity retrieval, prefer `show` over `search`:

```bash
asobi show "project-x:session" "UserPreferences"
```

`search` accepts an optional `--limit` argument. Omit it for the default top 100 matches; set it explicitly for larger ranked exports.
