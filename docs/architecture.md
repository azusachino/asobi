# Rosemary: Architecture & Design

## Why this exists

LLM agents lose context between sessions. The `@modelcontextprotocol/server-memory` server solves this but runs as a Node.js process with in-memory state — restart it and the graph is gone. Rosemary stores the graph in a local SQLite file: durable, zero-dependency, instant access.

---

## Storage tiers

```
┌────────────────────────────────────────────────────────┐
│                    rosemary.db (libSQL)                 │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Graph tier (hot)                               │   │
│  │  mcp_entities · mcp_observations · mcp_relations│   │
│  │  mcp_obs_fts (FTS5 virtual table)               │   │
│  └─────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Document tier (topics)                         │   │
│  │  topics · topics_fts · sessions · chunks        │   │
│  │  idx_chunks_vector (libSQL vector index)        │   │
│  └─────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────┘
           │
           │  compact --sync-only
           ▼
┌──────────────────────┐
│  .rosemary/topics/   │
│  *.md (cold storage) │
└──────────────────────┘
```

### Graph tier (hot)

All context-sharing operations live here. Three tables:

- `mcp_entities` — name (PK), entity_type, timestamps
- `mcp_observations` — id (UUID), entity_name (FK), content, created_at
- `mcp_relations` — (from_entity, to_entity, relation_type) composite PK, cascading FK deletes

Plus `mcp_obs_fts` — a FTS5 virtual table that mirrors `mcp_observations.content`. Kept in sync by three triggers (`mcp_obs_ai`, `mcp_obs_ad`, `mcp_obs_au`).

### Document tier (cold)

Ingested Markdown files chunked, embedded, and stored in the same libSQL database for semantic search. This tier is **optional** — graph operations never touch it. Only `ingest`, `query`, and `compact` initialize the vector search capabilities and the fastembed model.

The tier is also compile-time optional. The default Cargo build excludes
fastembed, token splitting, and directory ingest dependencies. Build with
`--features documents` to enable `ingest`, `query`, and `compact`.

---

## Why FTS5, not vector search, for `search-nodes`

The graph tier's `search-nodes` uses FTS5 (SQLite Full-Text Search 5), not the neural embedding model. Here's why:

### Startup cost

| Path                       | What happens at startup                          | Typical latency |
| -------------------------- | ------------------------------------------------ | --------------- |
| Graph CLI (`search-nodes`) | Open SQLite file (~1ms mmap)                     | **<10ms total** |
| Vector CLI (`query`)       | Load ONNX model (~100MB), init inference threads | **3–30s**       |

FTS5 is a data structure at rest in the `.db` file — b-trees stored as shadow tables. There is no service to start, no model to load. The OS page cache means repeated searches on a warm machine are pure RAM reads.

Vector search requires a neural network to embed the query text before searching. The model load cost is unavoidable; it dominates the entire operation for short-lived CLI invocations.

### Precision for structured facts

Observations are factual text with precise terms: `"status: IN_PROGRESS"`, `"Uses libSQL v0.6"`, `"branch: feat/mcp-knowledge-graph"`. For this kind of content:

- **FTS5 with porter stemming** — `"run"` reliably finds `"running"`. False positive rate is low because the vocabulary is technical and intentional.
- **Vector search** — excellent for natural language proximity (`"fast database"` → finds `"high-performance storage"`), but adds semantic noise for structured facts. `"IN_PROGRESS"` and `"DONE"` might cluster together because they're both status words.

For context-sharing, you want precision. FTS5 delivers it without model bias.

### BM25 ranking

FTS5 scores results by BM25 (Best Match 25) — a classic IR ranking function that weighs term frequency against document frequency. An entity with both query words in its observations ranks above one with only one. This is sufficient for the expected dataset size (<10k observations per project).

---

## Path resolution

Rosemary looks for storage location in priority order:

1. `rosemary.toml` in the current directory (project-local, checked in)
2. `.rosemary/` directory in the current directory (project-local, gitignored)
3. XDG paths (`~/.local/share/rosemary/`, `~/.config/rosemary/`)

This means different projects keep separate graphs automatically — no namespace collisions between agents working in different repos.

---

## Startup cost by command

| Command            | Default build | Initializes DB | Initializes fastembed | Typical cold start |
| ------------------ | ------------- | -------------- | --------------------- | ------------------ |
| `create-entities`  | yes           | yes            | **no**                | ~5ms               |
| `add-observations` | yes           | yes            | **no**                | ~5ms               |
| `read-graph`       | yes           | yes            | **no**                | ~5ms               |
| `search-nodes`     | yes           | yes            | **no**                | ~5ms               |
| `open-nodes`       | yes           | yes            | **no**                | ~5ms               |
| `delete-*`         | yes           | yes            | **no**                | ~5ms               |
| `ingest`           | documents     | yes            | yes                   | 3–30s              |
| `query`            | documents     | yes            | yes                   | 3–30s              |
| `compact`          | documents     | yes            | yes                   | 3–30s              |

The lazy-init split is enforced in `main.rs` via `needs_vector()`. Graph commands never pay the model load cost, and default builds do not link the document-tier crates.

---

## MCP stdio server

`rosemary mcp` runs a JSON-RPC 2.0 server over stdin/stdout that implements the Memory MCP protocol:

1. Client sends `initialize` → server responds with protocol version `2024-11-05` and tool capabilities
2. Client sends `notifications/initialized` (no response — it's a notification)
3. Client sends `tools/list` → server responds with all 9 tool schemas
4. Client sends `tools/call` with `name` + `arguments` → server dispatches to the graph tier and responds with `content[{type, text}]`

This makes `rosemary mcp` a drop-in replacement for `@modelcontextprotocol/server-memory` in Claude Code:

```bash
claude mcp add rosemary -- rosemary mcp
```

The MCP path reuses the same libSQL operations as the CLI commands — no separate code path.

---

## Performance headroom

The current implementation is correct and fast for typical use. `search-nodes`
defaults to top 100 matches; pass an explicit limit when you really need a
larger ranked export. Known improvement opportunities, in order of impact:

### 1. WAL journal mode _(easy, ~5 lines)_

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

SQLite's default rollback journal serializes all readers behind writers. WAL (Write-Ahead Logging) allows concurrent readers alongside one writer. Relevant when multiple agent processes write simultaneously (e.g., two sessions on the same project). Add to `init_db()`.

### 2. `FxHashSet` for deduplication _(easy, ~3 lines)_

`mcp_search_nodes` uses `Vec::contains` to deduplicate entity names — O(n) per check. Replace with `rustc-hash::FxHashSet` for O(1). Only matters at >100 matched entities, but it's a mechanical improvement.

### 3. Batch INSERT for observations _(medium)_

`mcp_create_entities` and `mcp_add_observations` insert one observation at a time in a loop. Multi-row INSERT or a prepared statement with a transaction wrapper would reduce per-row overhead significantly for bulk loads.

### 4. Parallel FTS + LIKE queries _(hard)_

The two search paths in `mcp_search_nodes` are sequential. With a connection pool (e.g., `bb8` + libsql), they could run concurrently via `tokio::join!`. The gain is small for <1k entities but meaningful for large graphs.

### 5. `compact` without re-embedding _(medium)_

`compact` always re-embeds every entity into libSQL. It could skip entities whose Markdown file hash hasn't changed since last sync. Add a `content_hash` column to `topics`.

---

## Schema diagram

```
mcp_entities          mcp_observations          mcp_relations
─────────────         ────────────────          ─────────────
name (PK)  ◄──── FK ─ entity_name              from_entity (FK)
entity_type           id (UUID PK)              to_entity (FK)
created_at            content          ◄─ FTS5  relation_type
updated_at            created_at       mcp_obs_fts

topics                topics_fts (FTS5)         sessions         chunks
──────                ─────────────────         ────────         ──────
id (PK)               title                     id (PK)          id (PK)
title                 body                      summary          topic_id (FK)
file_path                                       file_path        embedding (F32_BLOB)
body                                                             text
```
