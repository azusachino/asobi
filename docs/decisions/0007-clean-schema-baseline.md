---
id: 0007
title: "0007. A clean schema baseline for 0.8; pre-0.8 graph files are refused"
date: 2026-09-27
status: accepted
tags: [storage, schema, v0.8]
related: [0005-remote-server.md, 0006-tasks-replace-sessions.md, 0008-async-storage-on-sqlx.md]
---

## Context

The SQLite schema carries every generation's history: `upgrade_to_v5` drops tables from the MCP-memory and libSQL/Turso eras and converts old files to incremental auto-vacuum with a one-time `VACUUM`; `upgrade_to_v6` drops the skills table and skill entities; `upgrade_to_v7` and the v8 step rebuild search indexes. Each step exists only so an old file keeps opening.

0.8 changes who holds the graph ([0005](0005-remote-server.md)): the server starts empty and nothing is migrated to it. The remaining reason to keep the upgrade chain is local graph files created by earlier versions, and the owner has chosen not to carry them forward.

The schema also has a dead column and a derived value computed the long way. `asobi_entities.updated_at` is written once at insert and never again. "Last activity", which retention and abandonment ([0006](0006-tasks-replace-sessions.md)) both depend on, is recomputed on every sweep as a `MAX` over the entity, its observations and its truths.

## Decision

0.8 starts a clean schema baseline, version 9.

- **One creation path, no upgrade chain.** `upgrade_to_v5`, `upgrade_to_v6`, `upgrade_to_v7`, the v8 index-rebuild step and the legacy-table knowledge behind them are deleted. A new file is created directly at version 9, still switching on incremental auto-vacuum before the first write, since that is a property of a new file rather than a migration.
- **Pre-0.8 files are refused.** Opening a file whose `user_version` is non-zero and not 9 fails with an error that names the path, says the file was created by an Asobi version before 0.8 (or after this one, for a higher version), and says to move it aside to start a new graph. Nothing in the file is touched. There is no automatic move and no silent restart.
- **`last_activity` is a column.** `asobi_entities.updated_at` becomes `last_activity`, set at creation and kept current by triggers on every insert, update and delete of the entity's observations and truths. Relations do not count as activity. Retention and abandonment read `e.last_activity` instead of recomputing it, so the definition lives in one place: the schema.

Amended by [0008](0008-async-storage-on-sqlx.md): the baseline is carried by the first sqlx migration rather than `PRAGMA user_version = 9`; the refusal of pre-0.8 files and the `last_activity` design are unchanged.

## Consequences

- Every existing local graph must be moved aside once, deliberately, before 0.8 can open a graph at that path. Their content is not carried into 0.8 in any form.
- The storage code loses its migration history; tests that exercised old-file upgrades are deleted, and a test for the refusal replaces them.
- Sweeps read one column instead of running three correlated subqueries per entity.
- A future schema change starts a new, short upgrade chain from 9, or another baseline decision like this one.
