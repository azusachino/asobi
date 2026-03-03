# AGENTS

## Project Overview

Personal Knowledge Base CLI with hybrid Markdown/libSQL storage. Rosemary acts as a reactive memory for users and agents to store, relate, and recall technical concepts during conversations.

## Tech Stack & Architecture

- **Language:** Rust (Edition 2024), Python 3.14 (Scripts)
- **Database:** libSQL (SQLite3 protocol) for structured relations and vector gists
- **Storage:** Local Markdown files (`kb/topics/`) for durable content
- **CLI:** `clap` for command parsing
- **Task Management:** `mise` for tool management, `Makefile` for tasks, `uv` for Python environments

## Build, Run & Test

- **Rust:** `cargo build`, `cargo run`, `cargo test`
- **Python:** `uv run scripts/example.py`
- **Tasks:** `make build`, `make test`, `make fmt`, `make lint`

## Coding Conventions

- **Naming:** `snake_case` for functions, variables, and modules. `PascalCase` for types and traits.
- **Error Handling:** Use `anyhow::Result` for application-level errors and `thiserror` for library-level errors.
- **Conventions:** Follow standard Rust idioms (clippy).
- **Python:** Use `uv` for dependency management and Python 3.14.

## Key Files & Entry Points

- `src/main.rs`: CLI entry point and command matching.
- `src/db.rs`: libSQL schema and database initialization.
- `src/kb.rs`: Markdown ingestion and file management.
- `scripts/`: Python utility scripts.
- `kb/topics/`: Source of truth Markdown files.

## Quality Standards

- `cargo fmt` must be run on all Rust code.
- `cargo clippy` must pass with no warnings.
- `cargo test` must pass.
- Markdown files must follow basic formatting rules.
