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

2. Store context as you work:
   ```bash
   rosemary add-observations "my-task" "Decided to use WAL mode for concurrency"
   ```

3. Search for context later:
   ```bash
   rosemary search-nodes "WAL"
   ```

## Development

- **Task Runner**: `make` (Nix-wrapped)
- **Checks**: Run `make check` to verify formatting, linting, and tests.

See [`docs/usage.md`](docs/usage.md) for the full CLI reference.
