# Project Context: Rosemary

## Overview

A personal knowledge base CLI designed for agent-assisted learning and recall.

## Hard Rules

- **Markdown First:** All topic content must be stored in `kb/topics/` with YAML frontmatter.
- **Local-First:** Use libSQL (`rosemary.db`) and LanceDB (`data/lancedb`) locally.
- **Slugified Paths:** All file names and DB keys must use URL-safe slugs.

## Tech Stack

- Rust (Edition 2024)
- libSQL (SQLite3 protocol)
- LanceDB (Vector Store)
- Clap (CLI)
