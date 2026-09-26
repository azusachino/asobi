---
id: 0005
title: "0005. Shared graph through asobi serve and a JSON-RPC remote backend"
date: 2026-09-26
status: proposed
tags: [storage, api, server, rpc, v0.8]
related: [0001-sqlite-only-v2-rewrite.md, 0002-why-rusqlite.md, 0004-remove-skills.md]
---

## Context

One user runs agents on several devices, and those agents need one shared memory: the same entities, truths, observations, and task claims. Today every device has its own SQLite file, so each agent sees only its own graph.

Options considered (full comparison in harus-workstation's research note `2026-09-26-asobi-go-valkey-shared-context-feasibility.md`):

| Option | Why not |
| --- | --- |
| Rewrite in Go | Discards verified code for no capability gain. |
| etcd | Built for small config and leases: no search, request and database size limits, quorum-oriented. |
| Valkey (AOF) as a second backend | Reimplements every `v2` method in a key-value model; no BM25 or stemming without a module; a second storage engine to maintain forever, the cost [0001](0001-sqlite-only-v2-rewrite.md) removed. |
| Clients connect directly to PostgreSQL | A second storage implementation with different search ranking, and every client binary touches the shared schema, so version skew across devices can corrupt it. |
| **One server owns the SQLite file; clients call it** | Chosen. |

[0001](0001-sqlite-only-v2-rewrite.md) anticipated this: "any future long-lived server/daemon mode would need to reintroduce concurrency at the process level, … not resurrect async traits."

## Decision

### Shape

One binary, two ways to run:

```text
local (default)   asobi ──► SqliteStore ──► data_dir/asobi.db
remote            asobi ──► RemoteStore ──HTTP──► asobi serve ──► SqliteStore
```

- **`asobi serve --listen <addr:port>`** is a long-lived process that owns one `SqliteStore` and answers JSON-RPC.
- **`RemoteStore`** implements the same `v2` traits (`GraphStore`, `SearchStore`, `MaintenanceStore`, `TaskStore`) by sending one RPC per trait call. Commands keep depending on traits only, so every command, flag, and output is identical in both modes.
- **Configuration:** `remote = "http://host:port"` in `asobi.toml`, overridable by `ASOBI_REMOTE`. When set, the graph lives on the server and `data_dir` / `ASOBI_DATABASE_URL` are not used for it. `topics_dir` stays local, so `compact` writes its Markdown on the calling device. Local mode stays the default.

### RPC contract

- **Transport:** HTTP/1.1 `POST /rpc`, `Content-Type: application/json`, one [JSON-RPC 2.0](https://www.jsonrpc.org/specification) request object per HTTP request. No batches, no notifications: every request carries an `id`.
- **Methods:** one per `v2` trait method, named `<trait>.<method>` in camelCase. Params are a named object whose fields are the trait method's arguments; results are the method's return value, serialized with the existing camelCase serde types (`()` becomes `null`).

| Method | Params | Result |
| --- | --- | --- |
| `server.hello` | `{}` | `BackendInfo` |
| `graph.createEntities` | `{entities: EntityInput[]}` | `null` |
| `graph.addObservations` | `{observations: ObservationInput[], limit}` | `null` |
| `graph.createRelations` / `graph.deleteRelations` | `{relations: RelationInput[]}` | `null` |
| `graph.deleteEntities` | `{names: string[]}` | `null` |
| `graph.deleteObservations` | `{deletions: ObservationDeletion[]}` | `null` |
| `graph.deleteObservationById` | `{entityName, id}` | `null` |
| `graph.updateObservationById` | `{entityName, id, newContent}` | `null` |
| `graph.updateObservation` | `{entityName, oldContent, newContent}` | `null` |
| `graph.truthUpsert` | `{entity, key, value}` | `null` |
| `graph.truthDelete` | `{entity, key}` | `null` |
| `graph.readGraph` / `graph.readGraphFull` | `{}` | `Graph` |
| `graph.openNodes` | `OpenNodes` | `Graph` |
| `search.nodes` | `SearchQuery` (`filters` as `[key, value]` pairs) | `Graph` |
| `maintenance.stats` | `{}` | `Stats` |
| `maintenance.statsPerEntity` | `{}` | `[name, count][]` |
| `maintenance.purge` | `PurgeRequest` | `PurgeReport` |
| `maintenance.reset` | `{}` | always an `unsupported` error |
| `maintenance.capabilities` / `.health` / `.location` | `{}` | the server's `BackendCapabilities` / `BackendHealth` / `StorageLocation` |
| `tasks.dispatch` | `{task?, agent, observationLimit}` | `string \| null` |
| `tasks.claimNext` | `{agent}` | `string \| null` |

`ObservationInput`, `ObservationDeletion`, `OpenNodes` and `SearchQuery` gain `Serialize` (and `OpenNodes`/`SearchQuery` `Deserialize`) with the same camelCase convention; `asobi schema` publishes them.

- **Errors:** JSON-RPC's reserved codes for protocol failures (`-32700` parse, `-32600` invalid request, `-32601` unknown method, `-32602` invalid params). Each `ApiError` variant has a fixed code and a `data.kind`, so `RemoteStore` rebuilds the same variant and the CLI prints the same message as in local mode:

| `ApiError` | code | `data.kind` |
| --- | --- | --- |
| `NotFound` | `-32001` | `notFound` |
| `Conflict` | `-32002` | `conflict` |
| `Unsupported` | `-32003` | `unsupported` |
| `Unavailable` | `-32004` | `unavailable` |
| `Invalid` | `-32005` | `invalid` |
| `Backend` | `-32006` | `backend` |

- **Handshake:** before its first call, `RemoteStore` calls `server.hello` once per process and refuses to continue unless `apiVersion` equals its own `API_VERSION`. Only the server touches the schema, so schema version skew between devices cannot happen.

### Server behavior

- **Access:** no authentication. The server must only be reachable over the owner's tailnet; exposure is a deployment concern, and binding to a public interface is out of contract.
- **One request at a time.** SQLite has one writer; serial handling matches it and needs no pool or locking of its own.
- **`reset` is local-only.** Over RPC it is refused, so an agent on any device cannot wipe the shared graph. Run `asobi reset` on the server host against the file directly when that is intended.
- **Retention keeps running.** The CLI sweeps once per process before its first write; a server process lives for weeks, so `serve` sweeps before a write when the last sweep is more than a day old. `retention_days` is the server's setting.
- **No fallback when unreachable.** A remote-mode command whose server cannot be reached fails with `Unavailable`. No local writes, no later merge.

### Libraries

Server and client must stay synchronous, without an async runtime (the constraint from [0001](0001-sqlite-only-v2-rewrite.md)). Candidates are `tiny_http` for the server and `ureq` for the client; the choice is made against their documentation during implementation and recorded in the implementing PR.

## Verification

- `tests/backend_api_contract_test.rs` and `tests/concurrency_test.rs` run against `RemoteStore` with an in-process server on `127.0.0.1:0`, in addition to `SqliteStore`. SQLite-specific cases (migrations, `sqlite_master`, incremental vacuum) stay SQLite-only.
- Error round-trip: each `ApiError` variant survives server → wire → client unchanged.
- The handshake rejects a mismatched `apiVersion`; `maintenance.reset` over RPC is refused; an unreachable server yields `Unavailable`.

## Consequences

- Commands built from several trait calls (`tasks plan`, `tasks sync`, `tasks close`) are not atomic in remote mode: a failure part-way leaves the earlier calls applied. Accepted for now; a command that needs atomicity later gets a server-side method, not a client-side transaction.
- Each remote call is one HTTP round trip over the tailnet, so a command costs milliseconds per call instead of a local file access. Acceptable for a CLI.
- The server becomes a deployable (harus-k3s manifest, PVC, backup), owned by that repository, not this one.

## Roadmap: PostgreSQL behind the server

A `PgStore` implementing the `v2` traits may later replace `SqliteStore` **inside `asobi serve` only**. Clients are unaffected: they speak the RPC contract, not SQL. Differences to accept at that point: `ts_rank` instead of BM25 ranking, reported through `capabilities.keywordSearchKind`, and SQLite-specific tests staying SQLite-only.

Trigger, not a date: build it when the single SQLite file is a demonstrated bottleneck (write latency or size measured on the server), or when something other than Asobi needs SQL access to the graph.
