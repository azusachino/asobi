# CLI Response Contract

Asobi keeps command output simple: each machine-readable command returns its existing JSON payload directly. The compatibility promise is the discoverable JSON Schema, not an additional runtime envelope.

## Discovering a schema

```bash
asobi schema
asobi schema --command graph
asobi schema --command show
```

The index lists the schemas for the commands available in the current build. `--command NAME` prints one JSON Schema for that command's payload. The schema document carries `schemaVersion: 1`, independent from storage/export `apiVersion` and from the package version.

## Payloads

`graph`, `search`, and `show` return their graph payloads directly. Mutations using `--json` also return their existing receipt or affected-graph payload directly. Consumers should validate the payload against the matching schema; there is no `.data` wrapper.

Human-readable confirmations and errors remain on stderr. JSON export files remain the portable graph format consumed by `asobi import`.

## Version policy

- Additive optional fields do not require a schema-version bump.
- Removing, renaming, or changing the type of an existing field requires a schema-version bump.
- The schema version is independent from storage/export `apiVersion`.
- Update the schema derives, `asobi schema`, verifier, and this contract in the same change as any breaking payload change.
