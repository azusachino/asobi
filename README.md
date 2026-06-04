# Rosemary

Rosemary is a persistent, project-local knowledge graph CLI designed for AI agents. It allows agents to maintain memory, track session state, and persist knowledge directly within your project directory, ensuring context is preserved across sessions.

## Project Philosophy

- **Persistent**: Decisions and task state survive across conversations.
- **Local-first**: Data is stored inside your project, not an external server.
- **Agent-ready**: Optimized for CLI workflows, allowing agents to ingest, search, and compact knowledge.
- **Zero-latency**: Built on SQLite/FTS5 for sub-millisecond graph operations.

## Installation

### From Source (Cargo)
Requires Rust 1.85+ (Edition 2024):

```bash
cargo install --git https://github.com/azusachino/rosemary
```

## Quick Start

1. Initialize your project:
   ```bash
   rosemary init --local
   ```

2. Store context as you work (supports hierarchical naming like `ame:task-1`):
   ```bash
   rosemary add-observations "ame:mobile-support:task-1" "Decided to use WAL mode for concurrency"
   ```

3. Search for context later:
   ```bash
   rosemary search-nodes "WAL"
   ```

## Key Commands

- `rosemary --version`: Check the CLI version.
- `rosemary stats`: View graph statistics (entity, relation, and observation counts).
- `rosemary export -o graph.json`: Dump the graph to a JSON file.
- `rosemary import graph.json`: Restore graph data from a JSON file.
- `rosemary reset`: Completely wipe the graph (prompts for confirmation, bypass with `--force`).

## Development

- **Task Runner**: `make` (Nix-wrapped)
- **Checks**: Run `make check` to verify formatting, linting, and tests.

See [`docs/usage.md`](docs/usage.md) for the full CLI reference.
