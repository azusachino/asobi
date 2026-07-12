# CLI Response Contract

Asobi machine-readable CLI output uses a versioned envelope. This contract is
separate from the storage/export `apiVersion`.

## Success

Every graph read and every successful command using `--json` has this shape:

```json
{
  "schemaVersion": 1,
  "ok": true,
  "data": { "entities": [], "relations": [] }
}
```

The shape of `data` remains command-specific. Discover the exact schema with:

```bash
asobi schema
asobi schema --command show
```

Consumers should read graph fields through `.data`, for example
`.data.entities`.

## Errors

JSON-mode failures have no `data` field:

```json
{
  "schemaVersion": 1,
  "ok": false,
  "error": {
    "kind": "invalid_input",
    "message": "invalid request: ..."
  }
}
```

`error.kind` is stable and one of `not_found`, `conflict`, `invalid_input`,
`unsupported`, `unavailable`, `backend`, or `internal`. Human-readable mode
continues to write errors to stderr.

## Version policy

`schemaVersion` starts at `1` and is controlled by
`RESPONSE_SCHEMA_VERSION` in `src/response.rs`.

- Additive optional fields do not require a version bump.
- Removing, renaming, or changing the type of an existing field requires a
  version bump.
- Any envelope-shape change requires a version bump.
- `schemaVersion` is independent from the storage/export `apiVersion`.

The envelope and command schemas are released together. Update the schemas,
`verify_cli.py`, and this contract in the same change as any breaking response
change.
