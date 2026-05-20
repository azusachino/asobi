# rosemary

> 思い出、静かな力強さ — _memory, quiet strength_

A persistent knowledge graph CLI for humans and LLM agents. Store facts, decisions, and session state across projects and conversations — then retrieve them instantly with ranked full-text search.

## What it is

Rosemary maintains a graph of **entities** (named nodes), **observations** (facts attached to nodes), and **relations** (typed edges between nodes). Think of it as a local, offline, zero-latency alternative to `@modelcontextprotocol/server-memory` — except the storage lives in a SQLite file you own.

## Quick start

```bash
# Store a fact
rosemary create-entities "my-project" "project"
rosemary add-observations "my-project" "Uses libSQL — chosen for embedded + Turso remote parity"

# Link entities
rosemary create-entities "UserPreferences" "preference"
rosemary create-relations "my-project" "UserPreferences" "follows"

# Retrieve (FTS, stemmed, ranked)
rosemary search-nodes "libSQL"

# Full graph dump
rosemary read-graph
```

## Design

Two storage tiers, one file:

| Tier        | Technology          | Use for                                                |
| ----------- | ------------------- | ------------------------------------------------------ |
| Graph (hot) | libSQL + FTS5       | Entities, observations, relations — instant CLI access |
| KB (cold)   | LanceDB + fastembed | Semantic search over ingested Markdown files           |

Graph operations have no model startup cost. The FTS5 index is a b-tree inside the `.db` file — queried with a file open, not a server call. See [`docs/architecture.md`](docs/architecture.md).

## Documentation

- [`docs/architecture.md`](docs/architecture.md) — design decisions, storage tiers, FTS5 rationale, performance
- [`docs/usage.md`](docs/usage.md) — human workflows and agent integration
- [`docs/CHANGELOG.md`](docs/CHANGELOG.md) — release notes
- [`SKILL.md`](SKILL.md) — agent skill reference (full command API)

## Build

```bash
make build    # build rosemary CLI
make check    # fmt + clippy + tests
make test     # tests only
```

Requires Nix (`nix develop`) or a Rust toolchain with the dependencies in `Cargo.toml`.
