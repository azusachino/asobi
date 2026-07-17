# ADR 0003: Safe retention and agent-facing read UX

## Status

Proposed for the 0.6.x line.

## Context

Asobi serves two different jobs from one SQLite graph:

1. durable memory: projects, concepts, preferences, standards, and decisions;
2. operational coordination: sessions, task boards, dispatch notes, and skill installation state.

Those lifecycles must not be conflated. A graph can be persistent without making every operational log permanent, but silent age-based deletion of knowledge is not an acceptable default.

The agent-facing read path also has a separate constraint: `graph` and `search` must be indexes, not document dumps. Heavy observations and skill bodies are fetched only by an explicit `show`, `export`, or `skills show`.

## Decision

### Read tiers

| Operation | Default payload | Heavy content |
| --- | --- | --- |
| `graph` | entity identity, truths, observation counts, relations | never |
| `search` | same lean entity shape for matches | never |
| `show NAME` | selected entity observations and skill body | explicit |
| `show --with-ids` | selected observations with IDs | explicit |
| `export` / `backup` | complete archival state | explicit |
| `skills show NAME` | one skill body | explicit |

This keeps the common agent loop bounded by graph metadata rather than by the total amount of stored prose.

### Lifecycle classes

| Entity type | Default lifecycle | Automatic deletion |
| --- | --- | --- |
| `project`, `concept`, `preference`, `standard` | durable | never |
| `session` | volatile, one working context | only through explicit reset or approved stale cleanup |
| `task` | durable until terminal, then archival candidate | only when terminal and stale, with preview/apply safety |
| `skill` | managed by source synchronization | only explicit remove or source pruning |

The existing per-entity observation cap remains the immediate bounded-storage guard. It retains the newest observations and removes older ones only when a new observation is written. It is not a time-based deletion policy.

### Retention workflow

The future maintenance UX should be preview-first:

```text
asobi purge --dry-run --type session --older-than 30d
asobi purge --dry-run --type task --status DONE --older-than 90d
asobi purge --apply --type session --older-than 30d
```

Required safeguards:

- dry-run is the default;
- deletion requires `--apply`;
- the default allowlist is `session` and terminal `task` only;
- `project`, `concept`, `preference`, `standard`, and `skill` are rejected by the purge command unless a future explicit archive workflow is used;
- status and age filters are mandatory for task/entity deletion;
- output includes candidate names, status, last activity, observation count, and the exact deletion count;
- the deletion is one transaction and uses existing foreign-key/FTS cleanup;
- managed backups are recommended before the first applied purge.

“Last activity” is the newest of observation creation, truth update, or entity update timestamps. Creation time alone is insufficient because a long-lived session can still be active.

The initial implementation should purge operational observations or clearly terminal entities only after the preview contract is tested. It should not run implicitly during `graph`, `search`, `compact`, or application startup. Teams that want regular cleanup can schedule the explicit dry-run/apply workflow.

### Compaction and physical storage

`compact` remains a projection operation: it syncs durable knowledge to Markdown and prunes old session files. It is not a hidden database purge. Deleting rows reduces logical graph content and FTS entries, but SQLite may not immediately shrink the database file; backup/VACUUM maintenance is a separate concern.

### Shell completion

The binary generates completions from its own Clap command model:

```text
asobi completions bash|elvish|fish|powershell|zsh
```

This avoids a separately maintained completion grammar. Static command and flag completion is stable; dynamic entity-name completion is deliberately omitted, because a cached name list would become stale and add I/O to every shell tab. Users can search the graph explicitly when they need a current entity name.

## Consequences

- Agents can safely call `graph` on large graphs without loading stored prose.
- Users get a powerful cleanup path without surprise deletion of durable memory.
- Operational state can be reclaimed on a schedule, while durable state remains intentionally persistent.
- Retention requires timestamp-aware maintenance APIs and regression tests; it should not be implemented as an unreviewed SQL `DELETE` hidden in compaction.
- Completion scripts stay synchronized with the released binary and are easy to install in common shells.
