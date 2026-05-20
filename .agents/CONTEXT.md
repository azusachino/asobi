# Agent Rules

- **DO**: Use `make <target>` for all task execution.
- **DO**: At session start, load MCP entities if available; skip `CURRENT_TASK.md` when MCP active.
- **DO**: At session end, write state to `[rosemary]:session` MCP entity; do not write `CURRENT_TASK.md` when MCP active.
- **DO**: Update this file when architecture or conventions change.
- **DO**: Dispatch sub-agents for independent parallel tasks by default.
- **DON'T**: Commit without user confirmation.
- **DON'T**: Use plan mode (write-plan → execute-plan) for small, well-scoped tasks.
- **DON'T**: Install tools globally; use nix devShell or `make <target>`.

# Project Context: Rosemary

## Overview
Rosemary is both a learning-focused repo for async Rust and a personal knowledge base CLI for agent-assisted learning.

## Hard Rules
- **Markdown First**: All KB topic content must be stored in `kb/topics/` with YAML frontmatter.
- **Local-First**: Use libSQL (`rosemary.db`) locally for metadata, relations, and vectors.
- **Slugified Paths**: All file names and DB keys must use URL-safe slugs.
- **Clarity Over Cleverness**: Prioritize idiomatic Rust patterns and clear code for learning purposes.

# Tool Provisioning

- **Nix DevShell**: Primary tool source. Enter with `nix develop`.
- **Makefile**: Task runner wrapper ensuring `nix develop --command` is used when outside the shell.
- **Mise**: Fallback tool management.
- **UV**: Python tool and environment management.
- To add a new tool, add it to `flake.nix` in the `devShells.default.packages` list.
