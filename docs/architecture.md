# Asobi architecture

Asobi 0.8 ships two binaries over one async `api::v3` storage contract. The `asobi` CLI uses a local SQLite graph by default; with its optional `remote` feature and workspace configuration, it sends the same operations to `asobi-server`.

```text
asobi CLI
    |
api::v3 capability traits
    ├── local ── SqliteStore ── local SQLite graph
    └── remote ── RemoteStore ── HTTP ── asobi-server
                                         └── SqliteStore per named graph
```

## Workspace crates

- `asobi-core` owns domain types, errors, configuration, paths, and the transport-neutral `api::v3` traits and HTTP operation contract. It has no I/O stack.
- `asobi-storage` implements `SqliteStore` with sqlx, SQLite migrations, FTS5 search, and lifecycle sweeps. Only this crate depends on sqlx.
- `asobi` contains the CLI and `RemoteStore`. Its reqwest client is behind the opt-in `remote` feature, so a default local-only build does not include the HTTP client.
- `asobi-server` is a separate Tokio/Axum binary. It owns the server data directory and lazily opens one SQLite file per named graph.

Commands depend on API traits, not provider types. The CLI runs on a current-thread Tokio runtime; the server runs a multi-threaded runtime and calls the async storage API directly.

## Remote protocol and graph ownership

The remote client sends plain JSON over HTTP: `POST /v3/graphs/<graph>/<operation>`, with the operation request object as the body. Success returns JSON; failures use a non-2xx status and `{ "kind", "message" }`. There is no JSON-RPC envelope. Before accessing a graph, the remote CLI requests unversioned `GET /meta` and requires its supported API versions to include v3; the `/v3` path then selects that protocol. Build versions are diagnostic, not the protocol version. `GET /healthz` is a separate liveness probe. Neither GET opens a graph.

A workspace selects either local or remote mode for its entire graph. Remote workspaces name one server graph (default `asobi`); unrelated graphs cannot share entities or relations. The server accepts graph names matching `[a-z0-9-]+` and creates a valid graph on first use. Its required `--data-dir` is isolated from CLI config and XDG paths.

The CLI fails closed on connection failure, a two-second negotiation timeout, gateway 502/503/504, missing metadata, or an incompatible API version, without touching a local graph. Deploy the server before the 0.8.1 client: older servers do not serve `/meta`. Older clients still use the v3 routes on a new server. Later failures also return errors rather than switching backends. Remote `stats` shows the selected graph, endpoint, and counts, not the server's filesystem or SQLite details.

## Storage and lifecycle

`asobi-storage` uses sqlx with bundled SQLite/FTS5, WAL, foreign keys, and bounded busy timeouts. Queries are runtime-checked; schema 9 is established by the baseline migration. Asobi 0.8 refuses pre-0.8 graph files without modifying them; move those files aside to start a new graph. No migration or import is performed.

Tasks replace the former session workflow. Status is a truth, notes are observations, and task relationships connect work to epics. In local mode, abandonment and retention run once per process before its first write. `asobi-server` runs the same sweep hourly over its graphs: idle open tasks become `ABANDONED`, then finished tasks past retention are deleted. Active-parent protection and refreshed activity preserve the two-stage lifecycle.

`compact` projects durable graph entities to local Markdown topics; in remote mode, the graph is remote but `topics_dir` remains on the client. Asobi has no skills-management subsystem: install agent guidance from the maintained skills source with the external `skills` CLI.

## Verification

`make check` is the authoritative quality gate. It covers the shared storage contract on SQLite and RemoteStore, CLI and server integration tests, remote end-to-end tests, concurrency, formatting, linting, and benchmark compilation. SQLite-specific schema and vacuum cases stay local to `asobi-storage`.
