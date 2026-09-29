---
id: 0005
title: "0005. Shared graph through asobi-server and an HTTP remote backend"
date: 2026-09-26
status: accepted
tags: [storage, api, server, http, v0.8]
related: [0001-sqlite-only-v2-rewrite.md, 0002-why-rusqlite.md, 0004-remove-skills.md, 0006-tasks-replace-sessions.md, 0008-async-storage-on-sqlx.md, 0009-workspace-crates.md]
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

[0001](0001-sqlite-only-v2-rewrite.md) anticipated a server but expected it to stay synchronous. [0008](0008-async-storage-on-sqlx.md) revisits that: storage becomes async on sqlx, and the server below is built on it.

## Decision

### Shape

Two binaries; the CLI runs in one of two modes:

```text
local (default)   asobi ──► SqliteStore ──► data_dir/asobi.db
remote            asobi ──► RemoteStore ──HTTP──► asobi-server ──► SqliteStore per graph name
```

- **`asobi-server --listen <addr:port> --data-dir <path>`** is a separate binary ([0009](0009-workspace-crates.md)): a long-lived process that holds **named graphs**, one SQLite file per graph name in its data directory, and answers the HTTP protocol below. Its data directory is required and its own: it never falls back to the CLI's `asobi.toml` or XDG location, so a server and a local CLI on one host cannot end up sharing a graph file.
- **`RemoteStore`** implements the same `v3` traits (`GraphStore`, `SearchStore`, `MaintenanceStore`, `TaskStore`; see [0008](0008-async-storage-on-sqlx.md)) by sending one HTTP request per trait call. Commands keep depending on traits only, so every command, flag, and output is identical in both modes. Remote mode is compiled in only with the CLI's `remote` feature ([0009](0009-workspace-crates.md)).
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

### HTTP protocol

- **Transport:** plain JSON over HTTP/1.1. One call is `POST <remote>/v3/graphs/<graph>/<operation>` with `Content-Type: application/json`; the body is the operation's request object, and a success is HTTP 200 whose body is the result. The `v3` prefix is `API_VERSION`, so a later server can serve two versions side by side. "Method" always means the HTTP verb; what a call does is its **operation**. There is no envelope: HTTP already pairs each response with its request, and nothing here batches, streams, or sends notifications.
- **Operations:** one per `v3` trait method, named `<trait>.<method>` in camelCase (e.g. `graph.openNodes`). The request body is a named object whose fields are the trait method's arguments; a field left out takes the same default the CLI uses, never a zero that changes meaning (an omitted search `limit` is the CLI's default limit, not 0). Results are the method's return value, serialized with the existing camelCase serde types (`()` becomes a `null` body, still 200). An empty request body means `{}`.

| Operation | Request body | Result |
| --- | --- | --- |
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
| Unknown operation | 404 | `unknownOperation` |
| Body is not JSON, or does not match the operation's request | 400 | `badRequest` |
| Any method other than `POST` | 405 | `badRequest` |

JSON-RPC 2.0 was considered and not used: its `id`, batches and notifications serve nothing here, and it answers HTTP 200 for failures, hiding them from everything that reads HTTP.

- **No handshake; the first call is the probe.** The API version is the `v3` in the path, so there is no separate hello call. A response to a `/v3/…` request that is not a protocol response (e.g. a 404 without a `{kind, message}` body, from a server that does not serve v3) fails the command with "server does not speak API v3". The first call a process makes also decides reachability: if it cannot connect, times out (about two seconds), or a gateway in front of the server answers 502, 503 or 504, the server is unreachable and the command fails closed (see the 2026-09-29 amendment below); nothing has been written at that point. Once a call has succeeded, a later failure is an `Unavailable` error for that call, never a switch to the local graph.
- **Liveness:** `GET /healthz` answers 200 without touching any graph. It exists for container and cluster probes; clients never call it.
- **Messages:** the error `unsupported` from `maintenance.reset` is rebuilt on the client with a fixed message that says reset is not available over the network and names the alternative: `asobi reset` on the server host. Only the server touches the schema, so schema version skew between devices cannot happen.

### Server behavior

- **Access:** no authentication. The server must only be reachable over the owner's tailnet; exposure is a deployment concern, and binding to a public interface is out of contract.
- **Request logs:** emit structured method, path, status, and elapsed-time fields for every request, including health probes and routing failures. Do not log query strings or request bodies. Graph-open failures additionally include graph, operation, status, and protocol error details.
- **Concurrent requests, pooled storage.** Requests are served concurrently from one sqlx pool per graph ([0008](0008-async-storage-on-sqlx.md)). SQLite still has one writer: writes serialise through WAL and the busy timeout, and atomic operations (task claims, abandonment) use `BEGIN IMMEDIATE`.
- **`reset` is local-only.** Over HTTP it is refused, so an agent on any device cannot wipe a shared graph. Run `asobi reset` on the server host against the file directly when that is intended.
- **Sweeps run in the background.** The CLI sweeps once per process before its first write; a server process lives for weeks, so `asobi-server` runs the same sweeps (retention, and idle-task abandonment from [0006](0006-tasks-replace-sessions.md)) as a background task on a one-hour interval, over every graph it holds.

### When the server is unreachable

**Amended 2026-09-29:** A remote-configured command fails with a nonzero exit and an unavailable error if its first call cannot reach the server within about two seconds or receives a gateway 502/503/504. It never opens the local graph. The earlier 0.8 decision was to warn and fall back locally so agents could keep working; in practice that split task state across devices without synchronization. Use `asobi --local-graph <command>` only for deliberate, device-local work. Configured local work remains the default when no `remote` is selected. After a successful remote call, later failures remain errors and are never retried against a different backend.

The CLI keeps Asobi's own `stats` command for the actual selected target and graph counts, with `version` for client/server builds. Workspace config plus env overrides already express the local and shared-server use cases; there is no named context registry, `context` command or duplicate `info` alias. A new server adds its build version to the existing `maintenance.location` response; an old server remains readable with its version shown as `unknown`. The endpoint, graph, path ownership and schema version are separate facts.

### Libraries

The server is built on tokio, axum and hyper; the client on reqwest, one client per process so a command reuses its connection. Storage is sqlx ([0008](0008-async-storage-on-sqlx.md)).

## Verification

- `tests/backend_api_contract_test.rs` and `tests/concurrency_test.rs` run against `RemoteStore` with an in-process server on `127.0.0.1:0`, in addition to `SqliteStore`. SQLite-specific cases (migrations, `sqlite_master`, incremental vacuum) stay SQLite-only.
- Error round-trip: each `ApiError` variant survives server → wire → client unchanged.
- A server that does not serve `/v3` is reported as not speaking API v3; a 502/503/504 from a gateway on the first call fails closed; `maintenance.reset` over HTTP is refused; an unknown graph name is created, and an invalid one rejected.
- An unreachable server fails the command and creates no local graph; explicit `--local-graph` selects local work.

## Consequences

- **Amended for 0.8.1:** `tasks update` uses one additive `tasks.update` operation: its note and optional status commit atomically on the server. The old `tasks sync` remains available to old clients, with its multi-call behavior. `tasks plan` and `tasks close` still span several remote calls: a failure part-way leaves earlier calls applied; inspect the board before retrying. Other commands that need atomicity should gain a server-side operation, not a client-side transaction.
- Each remote call is one HTTP round trip over the tailnet, so a command costs milliseconds per call instead of a local file access. Acceptable for a CLI.
- The server becomes a deployable, owned by harus-k3s, not this repository: a Deployment with a PVC, a Traefik `IngressRoute` at `asobi.h.azusachino.com` (tailnet-only), and the existing SQLite backup CronJob.
- An outage blocks shared-graph commands rather than silently splitting memory; operators may explicitly choose a local graph for unrelated work.

## Roadmap: PostgreSQL behind the server

A `PgStore` implementing the `v3` traits may later replace `SqliteStore` **inside `asobi-server` only**. Clients are unaffected: they speak the HTTP protocol, not SQL. Differences to accept at that point: `ts_rank` instead of BM25 ranking, reported through `capabilities.keywordSearchKind`, and SQLite-specific tests staying SQLite-only.

Trigger, not a date: build it when the single SQLite file is a demonstrated bottleneck (write latency or size measured on the server), or when something other than Asobi needs SQL access to the graph.
