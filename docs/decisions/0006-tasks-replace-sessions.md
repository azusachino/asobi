---
id: 0006
title: "0006. Tasks replace sessions; idle tasks are abandoned automatically"
date: 2026-09-26
status: proposed
tags: [tasks, retention, lifecycle, v0.8]
related: [0003-retention-and-agent-ux.md, 0005-remote-server.md]
---

## Context

Agents recorded "where work stopped and what comes next" in one `session` entity per project, `[project]:session`, rewritten at every closeout. On one device that works. Once devices share a graph ([0005](0005-remote-server.md)), two devices working on the same project overwrite each other's session: the later closeout silently replaces the earlier one's next step.

Tasks already carry the same information with a better shape: a status truth, an append-only observation trail, one entity per unit of work, and unique names. The graphs in use held 137 task entities against 18 sessions; the session was a second, collision-prone record of what the tasks already said.

Open tasks also pile up. Retention ([0003](0003-retention-and-agent-ux.md), as revised in 0.7.0) only removes *finished* work; a task nobody closes stays open forever and clutters every "what is open" read.

## Decision

### No sessions

Asobi 0.8 removes `session` as a special entity type: retention no longer treats it as purgeable and `compact` no longer excludes it by name. Open tasks are the whole record of where work stands. The workflow guidance (the `asobi` skill in harus-skills) stops writing sessions and resumes from `tasks list`.

Task naming stays as it is: `tasks plan` keeps naming children `<epic>:task-N`, and standalone work uses `[project]:task:<name>`. Tasks are found by type, so no rename is needed for listing.

### Idle tasks are abandoned

An open task (any non-terminal status, or none) with **no activity for `abandon_days`** (default 7; `0` disables) becomes `ABANDONED`, with an observation recording that it was abandoned automatically after that many idle days. Activity is the entity's latest truth or observation change, the same measure retention already uses.

- **Epics are protected:** a task with an open `part_of` child is never abandoned by the sweep, since an epic's own entity goes quiet while its children are worked.
- **Two steps before deletion:** an abandoned task is terminal, so retention deletes it `retention_days` later (default 7). With defaults, an untouched task is visible as `ABANDONED` for a week and gone after two. Setting its status back revives it within that week.
- **Same rule in both modes:** local mode abandons in the existing per-process sweep before the first write; `asobi serve` runs it on its hourly background thread, with the server's `abandon_days`.

## Consequences

- Parallel devices on one project no longer overwrite each other: each works its own tasks, and a shared task records every device's notes as observations.
- A task paused for more than a week is abandoned, and deleted a week later unless revived. Long-lived backlog does not belong in Asobi; issues in the owning repository hold it.
- Existing `session` entities in local graphs become ordinary entities that nothing purges; delete them with `asobi rm` if wanted.
- This partly supersedes [0003](0003-retention-and-agent-ux.md) again: retention now closes idle work as well as removing finished work.
