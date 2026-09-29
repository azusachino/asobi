# CLI and HTTP Response Contracts

Asobi has two interfaces: the CLI's JSON output and the server's remote HTTP protocol. Both expose the same graph model; the CLI's discoverable JSON Schema applies to CLI command payloads.

## CLI schemas and payloads

```bash
asobi schema
asobi schema --command graph
asobi schema --command show
```

The index lists schemas available in the current build. `--command NAME` prints one JSON Schema for that command's payload. The schema document carries `schemaVersion: 2` in 0.8.1 because remote `stats` no longer includes database fields; it is independent from the storage API version (`v3`) and the package version.

`graph`, `search`, and `show` return their graph payloads directly. Mutations using `--json` also return their existing receipt or affected-graph payload directly. There is no `.data` wrapper. Human-readable confirmations and errors remain on stderr. The CLI schema describes command payloads; it is not a graph-file migration or import/export format.

`version` reports `clientVersion`, `serverVersion` (`not applicable` locally), and `apiVersion`. A remote version requires successful protocol negotiation; older servers without `/meta` fail with an upgrade error rather than returning `unknown`. `stats` retains counts and `mode` for all backends. Local output keeps `databasePath`, `journalMode`, `schemaVersion`, and `pathOwner: client`; remote output has graph, endpoint and server version but **omits** those local database fields, in JSON and human formats. An unreachable or incompatible remote fails closed. `--local-graph` selects the local store for one invocation; `init --local` still means initialize a workspace in the cwd.

`tasks-claim` and `tasks-update` each return a `TaskReceipt` under `--json`. `tasks.update` is an additive v3 operation taking a task name, optional notes and an optional status. Its resulting status response comes from one provider transaction: note-only updates preserve status, and note-plus-status updates commit or roll back together. An older server has no `tasks.update`; the unchanged `dispatch` and `sync` compatibility commands still speak their original operations.

The lazy-read shape is shared in local and remote mode: `graph` and `search` return entity identity, truths, observation counts, and relations without observation bodies; `show` returns requested observations and can add stable IDs with `--with-ids`.

## Remote HTTP protocol

A CLI built with the optional `remote` feature sends each API operation to its configured named graph using plain JSON over HTTP/1.1:

```text
POST <remote>/v3/graphs/<graph>/<operation>
Content-Type: application/json

<operation request object>
```

Before any graph request the new client calls unversioned `GET /meta` for `{serverVersion, supportedApiVersions}`. It requires v3 in the advertised list or refuses the command before writing. No graph is opened for metadata; an older server without this endpoint must be upgraded first. The `/v3` route remains available for old clients against new servers. A successful versioned operation returns HTTP 200 with the result as JSON. An empty request body means `{}`. A failure uses a non-2xx status and `{ "kind": "<kind>", "message": "<text>" }`. There is no JSON-RPC envelope. `GET /healthz` remains a liveness probe, not negotiation. For v3 backwards compatibility `maintenance.location` still carries the server-side path on the wire; only new CLI output hides it.

The graph name selects an isolated server-side graph; a valid name not yet held by the server creates it. The `asobi-server --data-dir` path contains one SQLite file per name. See [ADR 0005](decisions/0005-remote-server.md) and [the usage guide](usage.md#remote-workspaces) for operation names, errors, and configuration.

If negotiation cannot connect, times out, or receives gateway 502/503/504, the command fails without opening a local graph. After negotiation succeeds, a later failure is an error and does not switch backends. `maintenance.location` still includes `serverVersion` for v3 compatibility with old clients; the new CLI requires successful `/meta` negotiation before reading it.

## Version policy

- Additive optional fields do not require a schema-version bump.
- Removing, renaming, or changing the type of an existing field requires a schema-version bump.
- The schema version is independent from the storage API route version (`v3`) and package version.
- Update schema derives, `asobi schema`, its verifier, and this contract in the same change as any breaking CLI payload change.
