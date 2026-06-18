# Asobi: Architecture & Design

## Why this exists

LLM agents lose context between sessions. The `@modelcontextprotocol/server-memory` server solves this but runs as a Node.js process with in-memory state — restart it and the graph is gone. Asobi stores the graph in a local SQLite file: durable, zero-dependency, instant access.

---

## Storage tiers

```
┌────────────────────────────────────────────────────────┐
│                    asobi.db (libSQL)                   │
│                                                         │
│  ┌─────────────────────────────────────────────────┐   │
│  │  Graph tier (hot)                               │   │
│  │  asobi_entities · asobi_observations            │   │
│  │  asobi_relations · asobi_truths · asobi_skills  │   │
│  │  asobi_obs_fts (FTS5 virtual table)             │   │
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
│  .asobi/topics/   │
│  *.md (cold storage) │
└──────────────────────┘
```

### Graph tier (hot)

All context-sharing operations live here. Main tables:

- `asobi_entities` — name (PK), entity_type, timestamps
- `asobi_observations` — id (UUID), entity_name (FK), content, created_at
- `asobi_truths` — (entity_name, key) composite PK, value, updated_at
- `asobi_skills` — entity_name (PK, FK), body, source, version, installed_at
- `asobi_relations` — (from_entity, to_entity, relation_type) composite PK, cascading FK deletes

Plus `asobi_obs_fts` — a FTS5 virtual table that mirrors `asobi_observations.content`. Kept in sync by three triggers (`asobi_obs_ai`, `asobi_obs_ad`, `asobi_obs_au`).

### Document tier (cold)

Ingested Markdown files chunked, embedded, and stored in the same libSQL database for semantic search. This tier is **optional** — graph operations never touch it. Only `ingest`, `query`, and `compact` initialize the vector search capabilities and the fastembed model.

The tier is also compile-time optional. The default Cargo build excludes
fastembed, token splitting, and directory ingest dependencies. Build with
`--features documents` to enable `ingest`, `query`, and `compact`.

---

## Why FTS5, not vector search, for `search`

The graph tier's `search` uses FTS5 (SQLite Full-Text Search 5), not the neural embedding model. Here's why:

### Startup cost

| Path                  | What happens at startup                          | Typical latency |
| --------------------- | ------------------------------------------------ | --------------- |
| Graph CLI (`search`)  | Open SQLite file (~1ms mmap)                     | **<10ms total** |
| Vector CLI (`query`)  | Load ONNX model (~100MB), init inference threads | **3–30s**       |

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

Asobi looks for storage location in priority order:

1. `asobi.toml` in the current directory (project-local, checked in)
2. `.asobi/` directory in the current directory (project-local, gitignored)
3. XDG: a single `$XDG_DATA_HOME/asobi/` root (default `~/.local/share/asobi/`), holding the same `{data,config,topics,caches}` subtree as the project-local layout. `XDG_DATA_HOME` is honored on every platform, macOS included.

This means different projects keep separate graphs automatically — no namespace collisions between agents working in different repos.

---

## Startup cost by command

| Command            | Default build | Initializes DB | Initializes fastembed | Typical cold start |
| ------------------ | ------------- | -------------- | --------------------- | ------------------ |
| `new`              | yes           | yes            | **no**                | ~5ms               |
| `obs`              | yes           | yes            | **no**                | ~5ms               |
| `truth`            | yes           | yes            | **no**                | ~5ms               |
| `rm-truth`         | yes           | yes            | **no**                | ~5ms               |
| `graph`            | yes           | yes            | **no**                | ~5ms               |
| `search`           | yes           | yes            | **no**                | ~5ms               |
| `show`             | yes           | yes            | **no**                | ~5ms               |
| `rm` / `rm-obs`    | yes           | yes            | **no**                | ~5ms               |
| `unlink`           | yes           | yes            | **no**                | ~5ms               |
| `skills (list)`    | yes           | yes            | **no**                | ~5ms               |
| `skills install`   | yes           | yes            | conditional           | ~5ms or 3–30s      |
| `skills update`    | yes           | yes            | conditional           | ~5ms or 3–30s      |
| `skills remove`    | yes           | yes            | **no**                | ~5ms               |
| `skills show`      | yes           | yes            | **no**                | ~5ms               |
| `ingest`           | documents     | yes            | yes                   | 3–30s              |
| `query`            | documents     | yes            | yes                   | 3–30s              |
| `compact`          | documents     | yes            | yes                   | 3–30s              |

The lazy-init split is enforced in `main.rs` via `needs_vector()`. Graph commands never pay the model load cost, and default builds do not link the document-tier crates.

---

## Performance headroom

The current implementation is correct and fast for typical use. `search`
defaults to top 100 matches; pass an explicit limit when you really need a
larger ranked export. Known improvement opportunities, in order of impact:

### 1. WAL journal mode _(easy, ~5 lines)_

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

SQLite's default rollback journal serializes all readers behind writers. WAL (Write-Ahead Logging) allows concurrent readers alongside one writer. Relevant when multiple agent processes write simultaneously (e.g., two sessions on the same project). Add to `init_db()`.

### 2. `FxHashSet` for deduplication _(easy, ~3 lines)_

`search_nodes_with_limit` uses `Vec::contains` to deduplicate entity names — O(n) per check. Replace with `rustc-hash::FxHashSet` for O(1). Only matters at >100 matched entities, but it's a mechanical improvement.

### 3. Batch INSERT for observations _(medium)_

`create_entities` and `add_observations` insert one observation at a time in a loop. Multi-row INSERT or a prepared statement with a transaction wrapper would reduce per-row overhead significantly for bulk loads.

### 4. Parallel FTS + LIKE queries _(hard)_

The two search paths in `search_nodes_with_limit` are sequential. With a connection pool (e.g., `bb8` + libsql), they could run concurrently via `tokio::join!`. The gain is small for <1k entities but meaningful for large graphs.

### 5. `compact` without re-embedding _(medium)_

`compact` always re-embeds every entity into libSQL. It could skip entities whose Markdown file hash hasn't changed since last sync. Add a `content_hash` column to `topics`.

---

## Schema diagram

```
asobi_entities        asobi_observations        asobi_relations
─────────────         ──────────────────        ───────────────
name (PK)  ◄──── FK ─ entity_name               from_entity (FK)
entity_type           id (UUID PK)               to_entity (FK)
created_at            content          ◄─ FTS5   relation_type
updated_at            created_at       asobi_obs_fts

asobi_truths          asobi_skills
──────────            ──────────
entity_name (PK, FK)  entity_name (PK, FK)
key (PK)              body
value                 source
updated_at            version
                      installed_at


topics                topics_fts (FTS5)         sessions         chunks
──────                ─────────────────         ────────         ──────
id (PK)               title                     id (PK)          id (PK)
title                 body                      summary          topic_id (FK)
file_path                                       file_path        embedding (F32_BLOB)
body                                                             text
```
