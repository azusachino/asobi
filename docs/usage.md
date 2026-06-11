# Rosemary: Usage Guide

## For humans

### Installation

From source via cargo (Rust 1.85+ toolchain required for edition 2024):

```bash
cargo install --git https://github.com/azusachino/rosemary rosemary
```

Prebuilt binary via `cargo-binstall` (once GitHub releases are published):

```bash
cargo binstall rosemary
```

Or build locally:

```bash
git clone https://github.com/azusachino/rosemary && cd rosemary
make build            # graph/MCP CLI at ./target/debug/rosemary
make build-documents  # includes ingest/query/compact
```

### Workspace setup

Run once on a new machine — defaults to user-level XDG paths:

```bash
rosemary init
# created  ~/.local/share/rosemary
# created  ~/.local/share/rosemary/topics
# created  ~/.config/rosemary
```

No root or elevation needed: XDG dirs are inside `$HOME` and owned by the invoking user.

To keep a project's graph isolated and checked in alongside the code, use the local layout:

```bash
cd ~/code/my-project
rosemary init --local
# writes ./rosemary.toml + ./.rosemary/{data,topics,config}/
```

`rosemary.toml` (project-local mode):

```toml
data_dir   = ".rosemary/data"
config_dir = ".rosemary/config"
topics_dir = ".rosemary/topics"
```

Path resolution order at runtime: project-local `rosemary.toml` → project-local `.rosemary/` → XDG. Both `init` modes are idempotent.

Add `.rosemary/` to `.gitignore`; the `rosemary.toml` itself can be checked in.

### Common workflows

**Start a work session — load prior context:**

```bash
rosemary search-nodes "session"
rosemary open-nodes "my-project:session"
```
**Store a decision (supports hierarchical naming):**

```bash
rosemary create-entities "project-x:architecture" "project"
rosemary add-observations "project-x:architecture" "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts (preserves case and dots):**

```bash
rosemary create-entities "UserPreferences" "preference"
rosemary create-entities "CLAUDE.md" "reference"
rosemary create-relations "project-x" "UserPreferences" "follows"
```

**Search (supports FTS5 and segment matching):**

```bash
rosemary search-nodes "tokio"           # finds "tokio", "tokio-util", stemmed variants
rosemary search-nodes "mobile"          # finds "ame:mobile-support:task-1" (segment match)
rosemary search-nodes "auth*"           # prefix: matches "auth", "authentication", "authorize"
rosemary search-nodes "async AND error" # both words must appear
rosemary search-nodes "deploy OR ship"  # either word
rosemary search-nodes "auth" --limit 25 # override the default top 100 matches
```

Use `read-graph` for full export. `search-nodes` is intentionally top-K by default so a broad term does not accidentally return the whole graph.

**End a session — persist state:**

```bash
rosemary delete-observations "my-project:session" "status: IN_PROGRESS"
rosemary add-observations "my-project:session" "status: DONE"
rosemary add-observations "my-project:session" "next: implement FTS5 index"
rosemary add-observations "my-project:session" "last-updated: 2026-05-21"
rosemary compact  # syncs graph → markdown files for durable backup
```

**Inspect the full graph:**

```bash
rosemary stats                                # Quick count of entities, relations, observations
rosemary read-graph | jq '.entities[] | select(.entityType == "session")'
```

**Backup, Restore, and Reset:**

```bash
rosemary export -o backup.json                # Export the entire graph to JSON
rosemary import backup.json                   # Import entities and relations from a JSON backup
rosemary reset                                # Interactively clear the entire graph (use --force to bypass)
```

**Manage truths (structured key-value attributes):**

```bash
rosemary add-truth "project-x" "language" "rust"
rosemary delete-truth "project-x" "language"
```

**Manage skills (reusable workflows and knowledge):**

```bash
rosemary skills install https://github.com/azusachino/rosemary-skills --all
rosemary skills
rosemary skills update
rosemary skills remove rosemary-skills
```


**Ingest Markdown into the document tier (optional):**

These commands require a binary built with `--features documents`:

```bash
rosemary ingest ./notes/             # directory of .md files
rosemary query "async cancellation"  # semantic + FTS search
```

---

## For agents

### Overview

Rosemary is a persistent, project-local knowledge graph. Agents use it to:

- **Persist** decisions, task state, and user preferences across sessions
- **Share** context with other agents working on the same project
- **Resume** work without re-deriving context from git history or code

All operations are CLI commands. No server to start. No authentication. Latency is <10ms for graph operations.

### Session protocol

**At session start:**

```bash
# Option A: load a specific entity
rosemary open-nodes "<project>:session"

# Option B: keyword search
rosemary search-nodes "session"

# Option C: full graph (small projects)
rosemary read-graph
```

**During session — record facts as you learn them:**

```bash
rosemary add-observations "<project>" "Decided to use WAL mode for concurrent agent access"
rosemary add-observations "<project>:session" "status: IN_PROGRESS"
```

**At session end:**

```bash
# Update volatile state
rosemary delete-observations "<project>:session" "<old status line>"
rosemary add-observations "<project>:session" "status: DONE"
rosemary add-observations "<project>:session" "completed: implemented FTS5 search"
rosemary add-observations "<project>:session" "next: add WAL mode and entity_name index"
rosemary add-observations "<project>:session" "last-updated: 2026-05-21"

# Archive to markdown (durable, re-indexed)
rosemary compact
```

**Full session reset (next agent starts clean):**

```bash
rosemary delete-entities "<project>:session"
# Next agent creates it fresh
rosemary create-entities "<project>:session" "session"
```

### Entity naming conventions

| Pattern             | Type         | Purpose                                      |
| ------------------- | ------------ | -------------------------------------------- |
| `<project>:session` | `session`    | Volatile task state — reset each session     |
| `<project>:tasks`   | `task`       | In-progress task tracking                    |
| `<project>`         | `project`    | Stable project facts, architecture decisions |
| `UserPreferences`   | `preference` | Cross-project user habits                    |
| `CodingStyle`       | `standard`   | Commit format, indentation, etc.             |
| `ToolPreferences`   | `preference` | Mise, make, rtk, etc.                         |

### Output format

Rosemary operates under a **lazy-read contract** to minimize token overhead.

`read-graph` and `search-nodes` return a lazy JSON structure (excluding `observations` and skill `body`, only providing `truths` and `observationCount`):

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "truths": [
        ["key", "value"]
      ],
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
      "truths": [
        ["key", "value"]
      ],
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
rosemary add-observations "project-x:session" "status: BLOCKED"
rosemary add-observations "project-x:session" "next: Agent B should implement WAL mode in src/db.rs init_db()"
rosemary compact

# Agent B (start of session)
rosemary open-nodes "project-x:session"
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
rosemary open-nodes "project-x:session" "UserPreferences"
```

### MCP server mode (optional)

If your agent framework supports MCP stdio servers:

```bash
# Register once
claude mcp add rosemary -- rosemary mcp

# Protocol: MCP 2024-11-05, 11 tools
# Tools: create_entities, create_relations, add_observations,
#        delete_entities, delete_observations, delete_relations,
#        add_truth, delete_truth,
#        read_graph, search_nodes, open_nodes
```

The MCP server uses the same storage as the CLI — data written via `rosemary mcp` is immediately readable via `rosemary read-graph` and vice versa.

`search_nodes` accepts an optional `limit` argument. Omit it for the default top 100 matches; set it explicitly for larger ranked exports.
