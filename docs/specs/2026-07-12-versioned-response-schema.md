# Versioned CLI JSON Schemas (v0.5.2)

Status: implemented · Date: 2026-07-12 · Breaking: no runtime payload change

## Decision

Asobi's CLI keeps its existing machine-readable payloads. A schema is a
promise about those payloads, not a reason to add a transport envelope or a
new `.data` traversal layer.

The compatibility surface is:

```bash
asobi schema
asobi schema --command graph
```

The command index and each command schema carry `schemaVersion: 1`. This
version is independent from the package version and storage/export
`apiVersion`.

## Payload contract

`graph`, `search`, and `show` continue to return the graph payload directly.
Commands using global `--json` continue to return their existing affected-graph
or receipt payload directly. Export files remain unchanged and are still
consumed by `asobi import`.

Human-readable confirmations and errors remain on stderr. The schema promise
applies to successful machine-readable payloads; it does not require a
second runtime error envelope.

## Schemas

`schemars` derives cover the graph models, API data types, document recall
results, and promoted JSON receipts. `asobi schema --command NAME` emits one
JSON Schema for the command's direct payload. `asobi schema` emits an index of
all schemas available in the current build.

The CLI verifier invokes real commands and validates their direct stdout
payloads against the matching emitted schemas. It uses `fastjsonschema` via
`uv`; the dependency is verification-only and is not part of the Rust binary.

## Version policy

- Additive optional fields do not require a schema-version bump.
- Removing, renaming, or changing the type of an existing field requires a
  schema-version bump.
- The schema version is independent from storage/export `apiVersion`.
- Update schema derives, `asobi schema`, the verifier, and this document in the
  same change as any breaking payload change.

## Out of scope

- No mandatory response envelope.
- No `.data` wrapper for graph or mutation payloads.
- No change to human-readable output.
- No change to export/import snapshot contents.
