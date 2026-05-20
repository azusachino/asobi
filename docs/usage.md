# Rosemary: Usage Guide

## For humans

### Installation

```bash
git clone <repo>
cd rosemary
make build
# binary at ./target/debug/rosemary (or release with make build-release)
```

Add to `$PATH` or symlink to `~/.local/bin/rosemary`.

### Project-local setup

Create `rosemary.toml` in any project root to keep that project's graph isolated:

```toml
data_dir = ".rosemary/data"
config_dir = ".rosemary/config"
kb_dir = ".rosemary/topics"
```

Add `.rosemary/` to `.gitignore`. The `rosemary.toml` itself can be checked in — it contains no sensitive data.

### Common workflows

**Start a work session — load prior context:**

```bash
rosemary search-nodes "session"
rosemary open-nodes "my-project:session"
```

**Store a decision:**

```bash
rosemary create-entities "my-project" "project"
rosemary add-observations "my-project" "Switched from serde_yaml to toml crate — better error messages"
```

**Link related concepts:**

```bash
rosemary create-entities "UserPreferences" "preference"
rosemary create-relations "my-project" "UserPreferences" "follows"
```

**Search (supports FTS5 operators):**

```bash
rosemary search-nodes "tokio"           # finds "tokio", "tokio-util", stemmed variants
rosemary search-nodes "auth*"           # prefix: matches "auth", "authentication", "authorize"
rosemary search-nodes "async AND error" # both words must appear
rosemary search-nodes "deploy OR ship"  # either word
```

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
rosemary read-graph | jq '.entities[] | select(.entityType == "session")'
```

**Ingest Markdown into the semantic KB (optional):**

```bash
rosemary ingest kb/topics/        # directory of .md files
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
| `ToolPreferences`   | `preference` | Nix, make, rtk, etc.                         |

### Output format

Graph commands (`read-graph`, `search-nodes`, `open-nodes`) print JSON:

```json
{
  "entities": [
    {
      "name": "string",
      "entityType": "string",
      "observations": ["string", "..."]
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

For exact entity retrieval, prefer `open-nodes` over `search-nodes`:

```bash
rosemary open-nodes "project-x:session" "UserPreferences"
```

### MCP server mode (optional)

If your agent framework supports MCP stdio servers:

```bash
# Register once
claude mcp add rosemary -- rosemary mcp

# Protocol: MCP 2024-11-05, 9 tools
# Tools: create_entities, create_relations, add_observations,
#        delete_entities, delete_observations, delete_relations,
#        read_graph, search_nodes, open_nodes
```

The MCP server uses the same storage as the CLI — data written via `rosemary mcp` is immediately readable via `rosemary read-graph` and vice versa.
