# Rosemary

## Project Overview

Rosemary is a multi-purpose Rust project serving as:

1.  **Personal Knowledge Base CLI**: A reactive memory for users and agents to store, relate, and recall technical concepts using hybrid Markdown/libSQL storage.
2.  **Async Rust Masterclass**: A learning platform for mastering async Rust patterns, networking, and best practices through standalone examples and toolkit explorations.

## Tech Stack & Architecture

- **Language**: Rust (Edition 2024), Python 3.14 (Scripts)
- **Async Runtime**: `tokio` (full), `async-task`, `futures-lite`
- **Database**: libSQL (SQLite3 protocol) for structured relations and vector gists.
- **Storage**: Local Markdown files (`kb/topics/`) for durable content.
- **Networking**: `reqwest`, `tokio-util`, `tonic` (gRPC)
- **Serialization**: `serde`, `serde_json`
- **Channels**: `flume` (MPMC)
- **CLI**: `clap` for command parsing.
- **Tooling**: Nix + Makefile (Primary), `mise` for tool management, `uv` for Python environments.

The project is structured as:

- `src/`: Core library, KB management, and learning modules.
- `examples/`: Standalone async Rust samples.
- `kb/topics/`: Source of truth Markdown files for the Knowledge Base.
- `scripts/`: Python utility scripts.
- `benches/`: Performance benchmarks.

## Build, Run & Test

All daily operations are managed via `make`:

- `make fmt`: Format all code (Rust, Markdown, TOML, YAML).
- `make lint`: Run `clippy` with pedantic checks and Python linting.
- `make test`: Run all tests (Rust and Python).
- `make check`: Run format check, lint, and tests (CI baseline).
- `make build`: Build the Rosemary CLI.
- `make run-examples EXAMPLE=name`: Run a specific async example from `examples/`.

If you have Nix installed, these commands automatically run within the `nix develop` environment.

## Coding Conventions

- **Naming**: Standard Rust naming (snake_case for functions/variables, PascalCase for types).
- **Error Handling**: Prefer `anyhow` for top-level application logic and `thiserror` for library-style domain errors.
- **Testing**: Table-driven tests where appropriate. Integration tests in `tests/` or embedded unit tests.
- **Formatting**: `rustfmt` for Rust, `taplo` for TOML, `prettier` for MD/JSON/YAML.
- **Python**: Use `uv` for dependency management and Python 3.14.

## Key Files & Entry Points

- `src/main.rs`: CLI entry point and async masterclass entry.
- `src/lib.rs`: Library definitions, common utilities, and KB modules.
- `src/db.rs`: libSQL schema and database initialization.
- `src/kb.rs`: Markdown ingestion and file management.
- `Cargo.toml`: Project dependencies and configuration.
- `flake.nix`: Development environment definition.

## Quality Standards

- No warnings in `clippy`.
- All tests must pass.
- All code must be formatted with `make fmt`.
