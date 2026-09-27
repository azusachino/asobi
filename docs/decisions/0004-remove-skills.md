---
id: 0004
title: "0004. Remove skills management"
date: 2026-09-26
status: accepted
tags: [skills, scope, v0.8]
related: [0005-remote-server.md]
---

## Context

Skills management grew from a graph feature into a filesystem installer: declarative `[skills]` sources in `asobi.toml`, `skills install|sync|update|remove|show`, Markdown-only copying, shared-Markdown relocation, reference diagnostics, and a provenance manifest in `data_dir/skills.json`. By 0.7.3 it was about 2,600 of the ~5,500 lines in `src/` (`skills.rs`, `skills_config.rs`, `skill_resources.rs`, `skill_references.rs`, `cli/skills.rs`), plus its own CLI tests.

None of it touches the graph. It shares no code with storage, search, or tasks, and it duplicates a maintained tool: the [`skills` CLI](https://github.com/vercel-labs/skills) (`npx skills add <source> --skill <name> --agent universal`) installs selected skills into `.agents/skills/<name>/`, including from a subdirectory, a commit, or a local path. The only workspace that declared `[skills]`, harus-workstation, has moved to that CLI with a small reconcile script of its own.

Asobi's next change, a shared server (see [0005](0005-remote-server.md)), is about the graph. Carrying an unrelated half of the codebase through it is cost without benefit.

## Decision

Remove skills management from Asobi entirely in 0.8.0:

- delete the `skills` subcommand and `src/skills.rs`, `src/skills_config.rs`, `src/skill_resources.rs`, `src/skill_references.rs`, `src/cli/skills.rs`;
- delete the skills CLI tests (`cli_skill_references_test.rs`, `cli_skill_selection_test.rs`, `cli_shared_markdown_test.rs`, and skill cases elsewhere), and the `verify-skills-spec` gate with its `scripts/verify_skills_spec.py`;
- stop scaffolding the commented `[skills]` block in `init`;
- keep `src/frontmatter.rs`, which `compact` still uses.

An existing `[skills]` block in `asobi.toml` is **silently ignored**. `AsobiConfig` does not deny unknown fields, so this needs no code: the block simply has no reader.

## Consequences

- Asobi is a graph and task CLI again; `AGENTS.md`, `docs/usage.md` and `docs/architecture.md` lose their skills sections, and the `AGENTS.md` example install command moves to `npx skills add`.
- A stale `data_dir/skills.json` is left where it is. Asobi no longer reads or deletes it; removing it is the user's choice.
- Installed skill trees on disk are untouched. Nothing Asobi installed is removed by upgrading.
- The `asobi` skill in harus-skills documents `asobi skills …` and must be updated in that repository.
- Reintroducing skills management would be a new product decision, not a regression fix.
