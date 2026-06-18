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
asobi search-nodes "session"
asobi open-nodes "my-project:session"
```
**Store a decision (supports hierarchical naming):**

```bash
asobi create-entities "project-x:architecture" "project"
asobi add-observations "project-x:architecture" "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts (preserves case and dots):**

```bash
asobi create-entities "UserPreferences" "preference"
asobi create-entities "CLAUDE.md" "reference"
asobi create-relations "project-x" "UserPreferences" "follows"
```

**Search (supports FTS5 and segment matching):**

```bash
asobi search-nodes "tokio"           # finds "tokio", "tokio-util", stemmed variants
asobi search-nodes "mobile"          # finds "ame:mobile-support:task-1" (segment match)
asobi search-nodes "auth*"           # prefix: matches "auth", "authentication", "authorize"
asobi search-nodes "async AND error" # both words must appear
asobi search-nodes "deploy OR ship"  # either word
asobi search-nodes "auth" --limit 25 # override the default top 100 matches
```

Use `read-graph` for full export. `search-nodes` is intentionally top-K by default so a broad term does not accidentally return the whole graph.

**End a session — persist state:**

```bash
asobi delete-observations "my-project:session" "status: IN_PROGRESS"
asobi add-observations "my-project:session" "status: DONE"
asobi add-observations "my-project:session" "next: implement FTS5 index"
asobi add-observations "my-project:session" "last-updated: 2026-05-21"
asobi compact  # syncs graph → markdown files for durable backup
```

**Inspect the full graph:**

```bash
asobi stats                                # Quick count of entities, relations, observations
asobi read-graph | jq '.entities[] | select(.entityType == "session")'
```

**Backup, Restore, and Reset:**

```bash
asobi export -o backup.json                # Export the entire graph to JSON
asobi import backup.json                   # Import entities and relations from a JSON backup
asobi reset                                # Interactively clear the entire graph (use --force to bypass)
```

**Manage truths (structured key-value attributes):**

```bash
asobi add-truth "project-x" "language" "rust"
asobi delete-truth "project-x" "language"
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
asobi open-nodes "<project>:session"

# Option B: keyword search
asobi search-nodes "session"

# Option C: full graph (small projects)
asobi read-graph
```

**During session — record facts as you learn them:**

```bash
asobi add-observations "<project>" "Decided to use WAL mode for concurrent agent access"
asobi add-observations "<project>:session" "status: IN_PROGRESS"
```

**At session end:**

```bash
# Update volatile state
asobi delete-observations "<project>:session" "<old status line>"
asobi add-observations "<project>:session" "status: DONE"
asobi add-observations "<project>:session" "completed: implemented FTS5 search"
asobi add-observations "<project>:session" "next: add WAL mode and entity_name index"
asobi add-observations "<project>:session" "last-updated: 2026-05-21"

# Archive to markdown (durable, re-indexed)
asobi compact
```

**Full session reset (next agent starts clean):**

```bash
asobi delete-entities "<project>:session"
# Next agent creates it fresh
asobi create-entities "<project>:session" "session"
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

`read-graph` and `search-nodes` return a lazy JSON structure (excluding `observations` and skill `body`, only providing `truths` and `observationCount`):

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

`open-nodes` eagerly returns all `observations` and the skill `body` (if it's a skill entity):

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
asobi add-observations "project-x:session" "status: BLOCKED"
asobi add-observations "project-x:session" "next: Agent B should implement WAL mode in src/db.rs init_db()"
asobi compact

# Agent B (start of session)
asobi open-nodes "project-x:session"
# → reads: status BLOCKED, next action, last-updated
```

No files to pass, no state to reconstruct. The graph is the handoff.

### Search tips

`search-nodes` uses FTS5 with porter stemming. Practical implications:

- `search-nodes "run"` → finds entities with "running", "runner", "ran"
- `search-nodes "implement"` → finds "implementation", "implementing"
- `search-nodes "tokio async"` → finds entities with both words (ranked higher) or either word
- `search-nodes "UserPreferences"` → exact name match via LIKE fallback (entity has no observations)
- `search-nodes "AND AND"` → invalid FTS5 syntax, silently falls back to LIKE, returns empty
- `search-nodes "auth" --limit 500` → return more than the default top 100 matches

For exact entity retrieval, prefer `open-nodes` over `search-nodes`:

```bash
asobi open-nodes "project-x:session" "UserPreferences"
```

`search-nodes` accepts an optional `--limit` argument. Omit it for the default top 100 matches; set it explicitly for larger ranked exports.
