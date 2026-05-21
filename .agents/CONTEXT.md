# Agent Rules

- **DO**: Use `make <target>` for all task execution.
- **DO**: At session start, load MCP entities if available; skip `CURRENT_TASK.md` when MCP active.
- **DO**: At session end, write state to `rosemary:session` MCP entity; do not write `CURRENT_TASK.md` when MCP active.
- **DO**: Update this file when architecture or conventions change.
- **DO**: Dispatch sub-agents for independent parallel tasks by default.
- **DON'T**: Commit without user confirmation.
- **DON'T**: Use plan mode (write-plan → execute-plan) for small, well-scoped tasks.
- **DON'T**: Install tools globally; use nix devShell or `make <target>`.

# Project Context: Rosemary

## Overview

Rosemary is a persistent knowledge graph CLI for humans and LLM agents, with an optional document tier for semantic recall over Markdown. The repo also hosts standalone async Rust samples under `examples/`.

## Architecture

- **Graph tier (hot)**: libSQL at `.rosemary/data/rosemary.db` — entities, observations (FTS5 + porter stemming + BM25), relations. Single source of truth.
- **Document tier (cold)**: LanceDB at `.rosemary/data/lancedb/` plus `fastembed` — loaded only when `ingest` / `query` / `compact` runs. Graph-only commands skip the model entirely.
- **Workspace bootstrap**: `rosemary init` creates `.rosemary/{data,topics,config}/` and `rosemary.toml` in the current directory.
- **Path resolution**: project-local `rosemary.toml` > project-local `.rosemary/` > XDG.

## Hard Rules

- **Graph is canonical**: source of truth lives in libSQL. Markdown under `.rosemary/topics/` is a regenerable snapshot (`compact` rewrites it from the graph).
- **Slugified paths**: file names and topic IDs use URL-safe slugs.
- **Clarity over cleverness**: idiomatic Rust, no speculative abstractions.

# Tool Provisioning

- **Nix DevShell**: primary tool source. Enter with `nix develop`.
- **Makefile**: task-runner wrapper that prefixes `nix develop --command` when outside the shell.
- **mise**: language-runtime fallback only.
- **uv**: Python tooling and venvs.
- To add a tool: add it to `flake.nix` under `devShells.default.packages`.
