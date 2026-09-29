---
name: asobi
description: Use Asobi's local or shared graph to resume tasks, coordinate named agent claims, record handoffs, and recall decisions or pitfalls. Verify the selected graph before writing.
metadata:
  author: haru
  version: 3.1.0
---

# Asobi

Asobi is working memory, not the permanent backlog. Keep acceptance and review in the owning issue or PR, and durable decisions in the owning repository's docs. Use tasks for live execution; `task` entities expire after completion. Use `concept` for retained decisions and pitfalls. For exact flags and response contracts, see [Asobi's CLI reference](https://github.com/azusachino/asobi/blob/main/docs/usage.md).

## Before a write

1. Check `asobi --version`. This workflow uses 0.8.1 or newer. On an older CLI, check its help and be aware that 0.8.0 falls back to a local graph during remote outages; stop rather than writing to the wrong graph.
2. Run `asobi --json stats` from the **intended workspace**. Verify `mode`, `graph` (when remote), `endpoint`, `pathOwner`, and `databasePath`. The nearest `asobi.toml` wins: a nested checkout can select a different graph from its parent. `ASOBI_REMOTE` and `ASOBI_GRAPH` override that config. A remote outage fails closed in 0.8.1; `--local-graph` is only for deliberate device-local work.
3. For a resumed task, compare its recorded `branch` and `commit` truths with Git in the owning repository before trusting its `next` action. A server-owned path is not a client-local database file.

Reads (`show`, `search`, `tasks list`) emit JSON. `truth` succeeds silently unless `--json` is passed. Branch on exit codes, not success chatter; use `asobi schema --command NAME` for scripted responses.

## Execute and hand off

```bash
asobi tasks plan "project:epic" --objective "Deliver the change" \
  --task "Implement" --task "Verify"
asobi tasks list "project:epic"
asobi tasks claim "project:epic:task-1" --agent lead
asobi tasks update "project:epic:task-1" --note "tests pass"       # status unchanged
asobi tasks update "project:epic:task-1" --status REVIEW --note "ready for review"
asobi truth "project:epic:task-1" branch "$(git -C path/to/repo branch --show-current)"
asobi truth "project:epic:task-1" commit "$(git -C path/to/repo rev-parse HEAD)"
```

`claim` requires a task name; it atomically records ownership but does **not** launch an agent. Check `show <task> --expand depends_on` before claiming; creation order does not enforce dependencies. `update` requires `--note` or `--status`: note-only leaves the status alone, while note plus status commits together. Statuses include `READY_TO_DISPATCH`, `DISPATCHED`, `REVIEW`, `AWAITING_VERIFY`, `DONE`, and `BLOCKED_ON <dependency>`. Record what remains as an observation or `next` truth; do not mark `DONE` before verification. When every child is `DONE`, run `asobi tasks close "project:epic"`.

`tasks list` without an epic shows open work; `--all` includes finished tasks. `show` returns recent observations, while `show --limit 0` returns the full trail; `graph` and `search` are lean reads. Search for a decision before making a duplicate concept. Epics and tasks may be purged after retention; promote anything worth keeping to the issue, PR, or docs before closing.

`plan` and `close` span multiple calls in remote mode: after a failure, inspect `tasks list`/`show` before retrying. New `update` is one atomic operation and requires an 0.8.1 server. Legacy `dispatch` and `sync` remain for 0.8 scripts; `sync` implicitly selects `REVIEW` without `--status`. Use the new commands for handoffs.

## Recall and recovery

- `search` finds entities by topic or truth; use `show <name>` for observations, `show --limit 0 <name>` for the whole trail, and `show --expand part_of <epic>` for children. `graph` and `search` omit observation bodies to keep reads small.
- Use names like `project:task:slug` for standalone work, `project:decision:slug` or `project:pitfall:slug` for reusable findings, and `project:epic:task-N` for planned children. Check search before adding a duplicate. Current state belongs in truths; observations record the trail. Use `link <new> <old> supersedes` when replacing a decision. Shared preferences may live under `UserPreferences`, `CodingStyle`, or `ToolPreferences`.
- Open tasks idle for `abandon_days` (7 by default) become `ABANDONED`; an epic with open children is protected. Finished tasks are purged after `retention_days` (7 by default). Keep a continuing task active with a real note or truth update, and put any lasting decision in docs before retention removes the task. `purge --older-than N` previews; `--apply` deletes terminal tasks only.
- Observations are capped (200 by default). A local graph is one SQLite file; a remote graph is backed up on the server, where network `reset` is refused. An outage is not offline sync: 0.8.1 remote commands fail instead of switching to local.
