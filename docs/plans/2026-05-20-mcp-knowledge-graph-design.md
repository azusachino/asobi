# Design Spec: Rosemary MCP Knowledge Graph

**Date:** 2026-05-20
**Status:** DRAFT
**Author:** Gemini CLI

## 1. Overview

Rosemary will be extended with a Model Context Protocol (MCP) server interface that implements a full Knowledge Graph (Entities, Observations, Relations). This allows LLM agents to use Rosemary as a cross-session, cross-agent memory store.

## 2. Goals

- Implement the standard Memory MCP toolset (`create_entities`, `add_observations`, `create_relations`, etc.).
- Support an "Agent-First" high-efficiency write path (Hot Tier) using libSQL.
- Maintain the "Markdown-First" durable storage (Cold Tier) via background or manual compaction.
- Provide system-wide availability using XDG-compliant paths.

## 3. Architecture

### 3.1 Storage Model (Hybrid Tiered Storage)

- **Hot Tier (libSQL)**: Immediate storage for MCP tool calls. Handles frequent writes and complex relational queries for the graph.
- **Cold Tier (Markdown)**: Long-term archival and human-editable storage. Topics are stored in `.rosemary/topics/*.md`.
- **Vector Tier (LanceDB)**: Semantic search indexing across both tiers.

### 3.2 Path Management (XDG)

Rosemary will move away from local-folder storage to system-standard paths:

- **Data**: `~/.local/share/rosemary/` (DB, Vector store, Model cache)
- **Config**: `~/.config/rosemary/config.toml`
- **Topics Source**: `~/Documents/rosemary/topics/` (Default, configurable)

### 3.3 Data Schema (libSQL)

New tables to support the Graph:

- `mcp_entities`: `name` (PK), `entity_type`, `created_at`, `updated_at`.
- `mcp_observations`: `id` (UUID), `entity_name` (FK), `content`, `created_at`.
- `mcp_relations`: `from_entity` (FK), `to_entity` (FK), `relation_type`, `created_at`.

## 4. MCP Tools

The server will implement the following primitives:

1.  `create_entities`: Batch creation of nodes.
2.  `create_relations`: Link nodes with typed edges (active voice).
3.  `add_observations`: Append context to existing nodes.
4.  `delete_entities` / `delete_observations` / `delete_relations`: Graph maintenance.
5.  `read_graph`: Full export for agent context seeding.
6.  `search_nodes`: Semantic + Relational search.
7.  `open_nodes`: Targeted retrieval.

## 5. Implementation Phases

### Phase 1: Foundations & Paths

- Implement XDG path resolution.
- Create libSQL migrations for `mcp_entities`, `mcp_observations`, and `mcp_relations`.
- Refactor `db.rs` to support the new schema.

### Phase 2: MCP Server Core

- Implement the MCP protocol loop (stdio-based).
- Map MCP tool requests to libSQL operations.

### Phase 3: Document Integration & Compaction

- Implement `compact` logic to sync libSQL Graph data into Markdown files.
- Update `ingest` logic to ensure human-written Markdown updates the Graph state.

### Phase 4: CLI Refactor

- Update `main.rs` to provide a unified `rosemary` CLI with subcommands:
  - `rosemary mcp start`: Launch the server.
  - `rosemary [ingest|query|compact]`: Manage the document tier.

## 6. Success Criteria

- Passes full Memory MCP test suite (mocking tool calls).
- Database persists correctly in XDG paths.
- Changes made via MCP tools are visible in the Markdown files after compaction.
