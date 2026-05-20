# Design: Rosemary Personal Knowledge Base (v0.1.0)

**Date:** 2026-03-03
**Status:** Approved
**Topic:** Hybrid Markdown/libSQL/LanceDB Knowledge Base with Agent Integration

## 1. Overview

Rosemary will be transformed from an async playground into a personal knowledge base CLI. It serves as a "reactive memory" for agents to store, relate, and recall complex technical concepts discussed during conversations.

## 2. Architecture

The system uses a three-tier storage strategy to balance human readability, structured relationships, and semantic search.

### 2.1 Storage Layers

- **Markdown (Durability):**
  - Path: `kb/topics/{slug}.md`
  - Purpose: Long-form summaries and human-readable archives.
  - Format: Markdown + YAML frontmatter.
- **libSQL/SQLite (Graph/Metadata):**
  - Engine: `libsql` (local-first).
  - Purpose: Managing entities, specific "gists" (metadata/snippets), and their relationships.
  - Tables: `entities`, `relations`, `topics`.
- **LanceDB (Vector/Gists):**
  - Engine: `lancedb` (embedded).
  - Purpose: Semantic search and high-performance "recall" of tricky concepts.
  - Data: Chunks of content (gists) and their embeddings.

## 3. Components

### 3.1 The Rosemary CLI

A Rust-based binary providing the following interface:

- `ingest`: Process a new topic, create Markdown, index in libSQL, and embed in LanceDB.
- `recall`: Perform semantic search across the knowledge base.
- `relate`: Create a directed edge between two entities in the graph.
- `gist`: Retrieve high-signal snippets associated with an entity.

### 3.2 Agent Integration (MCP Pattern)

The CLI will expose operations that map to Model Context Protocol (MCP) memory standards (`create_entities`, `create_relations`, `add_observations`), allowing agents to autonomously update the knowledge base.

## 4. Implementation Strategy

1. **Scaffolding:** Update `Cargo.toml` with `libsql`, `lancedb`, and async dependencies.
1. **Core CLI:** Implement the basic command structure using `clap`.
1. **Storage Engines:** Initialize libSQL schemas and LanceDB tables.
1. **Ingestion Pipeline:** Build the logic to sync Markdown files with the databases.
1. **Vector Search:** Integrate an embedding model (e.g., via `candle` or a local API) for LanceDB.

## 5. Success Criteria

- Agent can successfully "ingest" a topic after a conversation.
- User can query the CLI for relations between concepts (e.g., "What is related to Pinning?").
- Vector search returns relevant "tricky" snippets from past discussions.
