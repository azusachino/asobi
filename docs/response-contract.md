# CLI and HTTP Response Contracts

Asobi has two interfaces: the CLI's JSON output and the server's remote HTTP protocol. Both expose the same graph model; the CLI's discoverable JSON Schema applies to CLI command payloads.

## CLI schemas and payloads

```bash
asobi schema
asobi schema --command graph
asobi schema --command show
```

The index lists schemas available in the current build. `--command NAME` prints one JSON Schema for that command's payload. The schema document carries `schemaVersion: 1`, independent from the storage API version (`v3`) and the package version.

`graph`, `search`, and `show` return their graph payloads directly. Mutations using `--json` also return their existing receipt or affected-graph payload directly. There is no `.data` wrapper. Human-readable confirmations and errors remain on stderr. The CLI schema describes command payloads; it is not a graph-file migration or import/export format.

`version` reports `clientVersion`, `serverVersion` (`unknown` for an older remote server, `not applicable` locally), and `apiVersion`. `stats` retains its existing counts and database fields plus `mode`, `pathOwner` (`client` or `server`), and optional remote graph, endpoint and server version. It reports the actual backend after connecting; an unreachable remote fails closed instead of returning local stats. Human output labels remote database paths as server-side. `--local-graph` selects the local store for one invocation; `init --local` still means initialize a workspace in the cwd.

The lazy-read shape is shared in local and remote mode: `graph` and `search` return entity identity, truths, observation counts, and relations without observation bodies; `show` returns requested observations and can add stable IDs with `--with-ids`.

## Remote HTTP protocol

A CLI built with the optional `remote` feature sends each API operation to its configured named graph using plain JSON over HTTP/1.1:

```text
POST <remote>/v3/graphs/<graph>/<operation>
Content-Type: application/json

<operation request object>
```

A successful response is HTTP 200 with the operation result as JSON. An empty request body means `{}`. A failure uses a non-2xx status and `{ "kind": "<kind>", "message": "<text>" }`. There is no JSON-RPC envelope and no `server.hello` handshake; `/v3` identifies the protocol version. `GET /healthz` returns 200 without opening a graph and is intended for liveness probes, not CLI negotiation.

The graph name selects an isolated server-side graph; a valid name not yet held by the server creates it. The `asobi-server --data-dir` path contains one SQLite file per name. See [ADR 0005](decisions/0005-remote-server.md) and [the usage guide](usage.md#remote-workspaces) for operation names, errors, and configuration.

If the first remote call cannot connect, times out, or receives gateway 502/503/504, the command fails without opening a local graph. After a remote call succeeds, a later failure is an error and does not switch backends. `maintenance.location` may include optional `serverVersion`, added by newer servers; old clients ignore the field and new clients show `unknown` when it is absent.

## Version policy

- Additive optional fields do not require a schema-version bump.
- Removing, renaming, or changing the type of an existing field requires a schema-version bump.
- The schema version is independent from the storage API route version (`v3`) and package version.
- Update schema derives, `asobi schema`, its verifier, and this contract in the same change as any breaking CLI payload change.
