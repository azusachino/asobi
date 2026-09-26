---
id: 0005
title: "0005. Shared graph through asobi serve and an HTTP remote backend"
date: 2026-09-26
status: proposed
tags: [storage, api, server, rpc, v0.8]
related: [0001-sqlite-only-v2-rewrite.md, 0002-why-rusqlite.md, 0004-remove-skills.md, 0006-tasks-replace-sessions.md]
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
remote            asobi ──► RemoteStore ──HTTP──► asobi serve ──► SqliteStore per graph name
```

- **`asobi serve --listen <addr:port>`** is a long-lived process that holds **named graphs**, one SQLite file per graph name in its data directory, and answers the RPC contract below over HTTP.
- **`RemoteStore`** implements the same `v2` traits (`GraphStore`, `SearchStore`, `MaintenanceStore`, `TaskStore`) by sending one RPC per trait call. Commands keep depending on traits only, so every command, flag, and output is identical in both modes.
- **Configuration:** two keys in `asobi.toml`, each overridable by its environment variable:
  - `remote = "https://asobi.h.azusachino.com"` (`ASOBI_REMOTE`) selects remote mode and the server;
  - `graph = "<name>"` (`ASOBI_GRAPH`) selects the graph on it, defaulting to `asobi`.

  When `remote` is set, the graph lives on the server and `data_dir` / `ASOBI_DATABASE_URL` are not used for it. `topics_dir` stays local, so `compact` writes its Markdown on the calling device. Local mode stays the default.
- **Graph names** match `[a-z0-9-]+`, because a name becomes a file on the server; anything else is rejected before touching the disk. Naming a graph the server does not hold yet **creates it**: there are only a few graphs, each set once in a config file.

### Data placement

The graph moves; files stay. In remote mode:

| Data | Lives |
| --- | --- |
| The graph: entities, observations, truths, relations, including task entities | **Server** |
| `retention_days`, `abandon_days` | **Server** config (the server runs the sweeps, see [0006](0006-tasks-replace-sessions.md)) |
| `observation_limit` | **Client** config, sent with each call |
| `compact` output under `topics_dir` | **Client**, generated from the remote graph |
| `asobi.toml` itself | **Client**, per workspace |

The choice is made **per workspace, for the whole graph**: a workspace whose `asobi.toml` sets `remote` uses the server for everything; one that does not stays local. There is no per-entity-type split. Mixing would put relations across two stores, merge search across two backends, and let an entity reference another that other devices cannot see. Separation between workspaces is by graph name: workspaces naming the same graph share it, and within a graph entities stay apart by their `<project>:*` prefixes. A device-private workspace simply does not set `remote`.

No existing graph is migrated: the server starts empty, and local graphs stay where they are.

### RPC contract

- **Transport:** plain JSON over HTTP/1.1. One call is `POST <remote>/rpc/<graph>/<method>` with `Content-Type: application/json`; the body is the params object, and a success is HTTP 200 whose body is the result. There is no envelope: HTTP already pairs each response with its request, and nothing here batches, streams, or sends notifications.
- **Methods:** one per `v2` trait method, named `<trait>.<method>` in camelCase. Params are a named object whose fields are the trait method's arguments; results are the method's return value, serialized with the existing camelCase serde types (`()` becomes a `null` body, still 200).

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

- **Errors:** a failure is a non-2xx status with the body `{"kind": "<kind>", "message": "<text>"}`. Each `ApiError` variant has a fixed status and `kind`, so `RemoteStore` rebuilds the same variant from `kind` and the CLI prints the same message as in local mode; the status makes failures visible to anything that only reads HTTP (logs, probes, `curl`).

| Case | Status | `kind` |
| --- | --- | --- |
| `ApiError::NotFound` | 404 | `notFound` |
| `ApiError::Conflict` | 409 | `conflict` |
| `ApiError::Invalid` (including an invalid graph name) | 422 | `invalid` |
| `ApiError::Unsupported` | 501 | `unsupported` |
| `ApiError::Unavailable` | 503 | `unavailable` |
| `ApiError::Backend` | 500 | `backend` |
| Unknown method | 404 | `unknownMethod` |
| Body is not JSON, or params do not match the method | 400 | `badRequest` |
| Any method other than `POST` | 405 | `badRequest` |

JSON-RPC 2.0 was considered and not used: its `id`, batches and notifications serve nothing here, and it answers HTTP 200 for failures, hiding them from everything that reads HTTP.

- **Handshake:** before its first call, `RemoteStore` calls `server.hello` once per process and refuses to continue unless `apiVersion` equals its own `API_VERSION`. Only the server touches the schema, so schema version skew between devices cannot happen.

### Server behavior

- **Access:** no authentication. The server must only be reachable over the owner's tailnet; exposure is a deployment concern, and binding to a public interface is out of contract.
- **One request at a time.** SQLite has one writer; serial handling matches it and needs no pool or locking of its own. The background sweep takes the same turn as a request.
- **`reset` is local-only.** Over RPC it is refused, so an agent on any device cannot wipe a shared graph. Run `asobi reset` on the server host against the file directly when that is intended.
- **Sweeps run in the background.** The CLI sweeps once per process before its first write; a server process lives for weeks, so `serve` runs the same sweeps (retention, and idle-task abandonment from [0006](0006-tasks-replace-sessions.md)) on a background thread, hourly, over every graph it holds.

### When the server is unreachable

A remote-mode command whose server cannot be reached within about two seconds **falls back to the workspace's local graph**, and prints a warning on stderr on every such command, stating that the command is running against the local graph and its writes will not reach the server. Nothing is merged later: writes made during an outage stay in the local graph. This keeps agents working through an outage at the known cost that their memory from that window is invisible to other devices.

### Libraries

Server and client must stay synchronous, without an async runtime (the constraint from [0001](0001-sqlite-only-v2-rewrite.md)). Candidates are `tiny_http` for the server and `ureq` for the client; the choice is made against their documentation during implementation and recorded in the implementing PR.

## Verification

- `tests/backend_api_contract_test.rs` and `tests/concurrency_test.rs` run against `RemoteStore` with an in-process server on `127.0.0.1:0`, in addition to `SqliteStore`. SQLite-specific cases (migrations, `sqlite_master`, incremental vacuum) stay SQLite-only.
- Error round-trip: each `ApiError` variant survives server → wire → client unchanged.
- The handshake rejects a mismatched `apiVersion`; `maintenance.reset` over RPC is refused; an unknown graph name is created, and an invalid one rejected.
- An unreachable server falls back to the local graph, with the warning on every command.

## Consequences

- Commands built from several trait calls (`tasks plan`, `tasks sync`, `tasks close`) are not atomic in remote mode: a failure part-way leaves the earlier calls applied. Accepted for now; a command that needs atomicity later gets a server-side method, not a client-side transaction.
- Each remote call is one HTTP round trip over the tailnet, so a command costs milliseconds per call instead of a local file access. Acceptable for a CLI.
- The server becomes a deployable, owned by harus-k3s, not this repository: a Deployment with a PVC, a Traefik `IngressRoute` at `asobi.h.azusachino.com` (tailnet-only), and the existing SQLite backup CronJob.
- An outage splits memory silently apart from the warning: what agents write during it stays on their device.

## Roadmap: PostgreSQL behind the server

A `PgStore` implementing the `v2` traits may later replace `SqliteStore` **inside `asobi serve` only**. Clients are unaffected: they speak the RPC contract, not SQL. Differences to accept at that point: `ts_rank` instead of BM25 ranking, reported through `capabilities.keywordSearchKind`, and SQLite-specific tests staying SQLite-only.

Trigger, not a date: build it when the single SQLite file is a demonstrated bottleneck (write latency or size measured on the server), or when something other than Asobi needs SQL access to the graph.
