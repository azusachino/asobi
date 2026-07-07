<div align="center">

# 🎮 asobi

**A persistent, project-local knowledge graph CLI for LLM agents.**

Keep memory, track session state, and share context across conversations — stored in a local, single-file libSQL/SQLite database.

[![CI](https://github.com/azusachino/asobi/actions/workflows/ci.yml/badge.svg)](https://github.com/azusachino/asobi/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/tag/azusachino/asobi?label=release&sort=semver)](https://github.com/azusachino/asobi/releases)
[![License: MIT](https://img.shields.io/github/license/azusachino/asobi)](LICENSE)
[![Built with Nix](https://img.shields.io/badge/built%20with-nix-5277C3?logo=nixos&logoColor=white)](https://nixos.org)
[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)

[![Last commit](https://img.shields.io/github/last-commit/azusachino/asobi)](https://github.com/azusachino/asobi/commits/main)
[![Stars](https://img.shields.io/github/stars/azusachino/asobi?style=social)](https://github.com/azusachino/asobi/stargazers)

</div>

---

## ✨ Features

- **Knowledge graph** — entities, append-only (capped) observations, and directed relations.
- **Truths** — durable `key→value` facts per entity for current state (`status`, `version`); status-as-truth makes a board a single `search --where status=…`.
- **Fast search** — `search` over FTS5 (porter stemming + BM25) with a substring fallback, plus `--where key=value` truth filters (the query term is optional).
- **Concurrency-safe** — WAL + `busy_timeout`, so a lead agent and dispatched sub-agents can write the same graph without lock errors.
- **Lazy reads** — `graph`/`search` return truths + counts; `show` returns the full body. Cheap to load, cheap on tokens.
- **Skills** — install reusable agent instructions from a git repo or local path.
- **Document tier** (optional, `--features documents`) — `ingest` + semantic `query` over Markdown.

## 📦 Installation

### From crates.io (recommended)

```bash
cargo install asobi
# with the optional document tier (semantic ingest + query):
cargo install asobi --features documents
```

### Prebuilt binary (cargo-binstall)

No compile — [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall) pulls the binary from the GitHub release:

```bash
cargo binstall asobi
```

### From source

```bash
cargo install --git https://github.com/azusachino/asobi
```

Or build locally with `make build` (graph CLI) or `make build-documents` (adds `ingest`/`query`/`compact`). Requires Rust 1.85+, Edition 2024.

## 🚀 Quick Start

```bash
asobi init                  # one-time setup (XDG); use --local for a project-scoped graph

# Store and recall context (names are hierarchical, e.g. ame:mobile-support:task-1)
asobi obs "my-project" "Decided to use WAL mode for concurrency"
asobi truth "my-project" "status" "in-progress"
asobi search "WAL"
asobi show "my-project" --with-ids
asobi update-obs "my-project" 1 "Decided to use WAL mode + busy_timeout for concurrency" --id
asobi rm-obs "my-project" 1 --id

```

## 💻 Common Commands

- `asobi graph` / `search <q>` / `search --where status=READY` / `show <name>... --expand part_of --with-ids` — read the graph (supports subtree expansions and sequential observation IDs).
- `asobi new <name> <type> --obs "..."` / `obs <name> "..."` / `update-obs <name> <old/id> <new> [--id]` / `rm-obs <name> <content/id> [--id]` — manage observations (supports updates and deletions by unique sequential IDs).
- `asobi truth <name> <key> <value>` / `rm-truth <name> <key>` — manage truths.
- `asobi skills install <src> --all` / `update` / `skills` / `skills show <name>` — manage skills (`--all` and `update` sync, pruning skills dropped upstream; `--select` is additive).
- `asobi stats` / `export -o graph.json` / `import graph.json` / `reset` — inspect & manage.

## 🔒 Sandboxed Environments

When running in sandboxed or restricted environments (such as Codex, Nix build sandboxes, or containerized runners), you might need to use a project-local workspace (`asobi init --local`), configure custom database paths (`ASOBI_HOME`, `ASOBI_DATABASE_URL`), adjust busy lock timeouts (`ASOBI_BUSY_TIMEOUT`), or force rollback journal mode (`ASOBI_JOURNAL_MODE=DELETE`) if shared memory (`-shm`) creation is unsupported on the underlying filesystem.

See the [Running in Sandboxed Environments](docs/usage.md#running-in-sandboxed-environments-codex-etc) section in the Usage Guide for more details.

## 🛠️ Development

- **Task runner**: `make` (Nix-wrapped). Run `make check` for fmt + lint + tests.
- See [`docs/usage.md`](docs/usage.md) for the full CLI reference, [`docs/workflow.md`](docs/workflow.md) for the day-to-day and task dispatcher workflow, and [`docs/architecture.md`](docs/architecture.md) for design.
