---
name: asobi
description: Use Asobi's persistent SQLite knowledge graph for session continuity, durable task dispatch, keyword recall, and reusable skills.
metadata:
  author: haru
  version: 2.4.0
---

# Asobi Skill

Use Asobi as the canonical shared state store. Session state, task state, decisions, pitfalls, and reusable skills belong in the graph rather than in chat-only notes or local todo files.

Resolve graph scope before reading or writing. Asobi searches ancestors for `asobi.toml`, then `.asobi/`, and otherwise uses shared XDG state; `ASOBI_HOME` overrides discovery. A nested repository can inherit its parent's graph. Follow the workspace's intended ownership and inspect the discovered configuration; changing directories into a submodule does not guarantee shared state. Create local state with `asobi init --local` only when requested.

Graph reads emit JSON on stdout; `asobi skills show` emits Markdown. Pass the global `--json` flag when a mutation's result needs parsing; human confirmations vary by command. Check `asobi --version` and command help against the installed CLI before relying on an unfamiliar operation.

## State model

- **Truth** — current state, such as `status`, `next`, `version`, or a date. Writing the same key updates it.
- **Observation** — append-only history, such as a completed session, implementation note, decision, or lesson.
- **Relation** — a directed connection between entities.

`graph` and `search` are lean reads: they return truths, observation counts, and relations without observation bodies or skill bodies. Use `show` for selected full content, `--with-ids` for observation IDs, and `--expand part_of` for an epic and its tasks.

Use `asobi schema --command NAME` when a scripted caller needs the exact response contract.

## Detect Asobi

Run `command -v asobi` once at session start. If unavailable, report that persistence is unavailable and continue independent work. If the task itself requires graph access, explain that dependency and request installation; do not claim that state was loaded or saved.

## Session start

In the selected graph, load the project and session plus any preferences stored there. Shared preferences may require a separate read from the configured shared graph. Current user instructions override recalled preferences; verify drift-prone state against Git and the relevant live system.

```bash
asobi show UserPreferences CodingStyle ToolPreferences "[project]" "[project]:session"
```

If an active epic is named in the session, load its board:

```bash
asobi tasks list "[project]:[epic]"
```

Load active project pitfalls without opening every entity:

```bash
asobi search "pitfall" --where status=active
```

Report only `[project]:pitfall:*` entities. Also run `git log --oneline -5` and flag stale context when recent feature commits are not reflected in the loaded state.

Briefly report the relevant last task and next action when a prior session exists. Surface applicable active pitfalls and discrepancies without dumping the graph or inventing continuity for an empty result.

## Session end

Save current session state as truths and append one completion observation:

```bash
asobi new "[project]:session" session
asobi truth "[project]:session" objective "[objective or active epic]"
asobi truth "[project]:session" status "[IN_PROGRESS|BLOCKED|REVIEW|DONE]"
asobi truth "[project]:session" remaining "[what remains]"
asobi truth "[project]:session" next "[single most important next action]"
asobi truth "[project]:session" last-updated "YYYY-MM-DD"
asobi obs "[project]:session" "completed YYYY-MM-DD: [finished work]"
```

Record durable project facts on `[project]`, cross-project facts on `UserPreferences`, `CodingStyle`, or `ToolPreferences`, and decisions or pitfalls in their dedicated entities. Check `asobi search "[topic]"` before adding a duplicate.

Use `asobi compact` only as requested maintenance. Installed versions may advertise session pruning as well as Markdown synchronization; check the installed version's implementation before assuming it only exports topics. Ordinary session closeout only needs the state writes above.

## Tasks — primary workflow

Use the task dispatcher for work that benefits from durable checkpoints or handoff. A small one-step task can use session closeout alone. Human-facing plans belong in the project's documentation; store status and a pointer in Asobi instead of maintaining two competing plans.

An epic is the objective; its child tasks are ordered dispatchable units. The dispatcher owns task creation, links, status transitions, dispatch notes, and closeout.

### Plan

Create a finite epic with concrete completion criteria. List tasks in dependency order; use explicit task names when dispatching, since creation order alone does not enforce dependencies:

```bash
asobi tasks plan "[project]:[epic]" \
  --objective "[what this epic delivers]" \
  --task "[first dispatchable task]" \
  --task "[next dispatchable task]"
```

Point the session at the epic:

```bash
asobi truth "[project]:session" objective "[project]:[epic]"
```

### List

Use the task board as the normal status read:

```bash
asobi tasks list "[project]:[epic]"
```

Use `asobi show "[project]:[epic]" --expand part_of` only when task observations or linked details are needed.

### Dispatch

Dispatch the intended task by name to avoid claiming unrelated work in the same graph:

```bash
asobi tasks dispatch "[project]:[epic]:task-N"
asobi tasks dispatch "[project]:[epic]:task-N" --agent "[agent]"
```

Dispatch claiming is atomic. It records a claim; it does not launch an agent. Follow the current workspace's delegation policy, use subagents for independent bounded work, and reserve worktrees or parallel sessions for explicit opt-in. `--agent` labels the claim for the chosen worker.

Before dispatch, read the task and search relevant lessons:

```bash
asobi show "[project]:[epic]:task-N" --expand depends_on
asobi search "[task title]"
```

### Sync

Record implementation or review notes and advance the task:

```bash
asobi tasks sync "[project]:[epic]:task-N" \
  --status REVIEW \
  --note "[files changed; verification result; review notes]"
```

Use `AWAITING_VERIFY` when a human or device check remains, and `DONE` only after the required verification is complete.

### Close

Close an all-DONE epic and promote a durable lesson when useful:

```bash
asobi tasks close "[project]:[epic]" \
  --lesson "[convention or decision learned]"
```

The task lifecycle is `READY_TO_DISPATCH → DISPATCHED → REVIEW → AWAITING_VERIFY → DONE`. Keep the next action in the session truth.

## Recall and decisions

Use SQLite FTS5/BM25 keyword search for graph recall:

```bash
asobi search "WAL concurrency"
asobi search "auth" --limit 500
asobi search --where status=READY
```

Use `show` for the full observations of selected entities. Use `graph` only when the full lean graph is required. Use `export` for portable graph handoff and `backup` for full-fidelity local recovery.

Record non-obvious decisions as concepts:

```bash
asobi new "[project]:decision:[slug]" concept
asobi obs "[project]:decision:[slug]" "decision: [chosen path]"
asobi obs "[project]:decision:[slug]" "context: [constraints]"
asobi obs "[project]:decision:[slug]" "consequences: [accepted trade-offs]"
asobi link "[project]:decision:[new]" "[project]:decision:[old]" supersedes
```

Record rejected approaches as active pitfalls:

```bash
asobi new "[project]:pitfall:[slug]" concept
asobi truth "[project]:pitfall:[slug]" status active
asobi truth "[project]:pitfall:[slug]" title "[short warning]"
asobi obs "[project]:pitfall:[slug]" "tried: [rejected approach]"
asobi obs "[project]:pitfall:[slug]" "why-it-failed: [cause]"
asobi obs "[project]:pitfall:[slug]" "do-instead: [working approach]"
```

## Skills

Use the skill library as the source of truth for installed skills. Prefer explicit non-interactive selection:

When `asobi.toml` declares `[skills]`, edit that selection and run `asobi skills sync` from its workspace root. Review additions, updates, and removals in the materialized skill files; commit them with the declaration when the repository tracks them. Sync can prune undeclared skills. Read changed instructions before using them. For installations without a declarative selection:

```bash
asobi skills install "[git-url-or-path]" --select skill-a skill-b
asobi skills install "[git-url-or-path]" --all
asobi skills update "[source]"
asobi skills show "[name]"
```

`--all` synchronizes a source and removes skills deleted upstream. `--select` is additive. Never hand-edit installed skill entities.

## Retention and recovery

Observations are capped at 200 per entity by default. Keep current state in truths and consolidate old observation trails when needed.

Preview stale operational records before applying retention:

```bash
asobi purge --type task --status DONE --older-than 90
asobi purge --type task --status DONE --older-than 90 --apply
```

Purge is restricted to terminal sessions and tasks; durable knowledge and skills are protected. It is never implicit.

Use SQLite backup/restore for local recovery and JSON export/import for portable handoff:

```bash
asobi backup --keep 5
asobi restore "/secure/asobi.db"
asobi export --scope "[project]:[epic]" --rationale -o handoff.json
asobi import handoff.json
```

Restore replaces live state; use it only for an explicitly requested recovery after confirming the target and backup. Export/import and purge are separate maintenance actions, not implicit session closeout steps.

Generate shell completions from the installed binary:

```bash
asobi completions bash|elvish|fish|powershell|zsh
```
