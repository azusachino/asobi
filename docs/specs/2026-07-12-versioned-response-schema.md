# Versioned Response Schema (v0.5.2)

Status: implemented · Date: 2026-07-12 · Breaking: yes (patch release to 0.5.2)

## Problem

Every CLI command prints a **bare** domain object straight from `serde`
(`main.rs` has ~12 independent `println!(serde_json::to_string_pretty(...))`
sites). There is:

- **No version discriminator** — an agent cannot tell which output contract it
  is parsing, so any field change is a silent break.
- **No consistent envelope** — `graph`/`search`/`show` emit `Graph`, `history`
  emits an array, `stats --json` and mutation receipts emit ad-hoc `json!{}`.
- **Inconsistent error channel** — errors go to stdout as `{"error": "..."}`
  only under `--json`, otherwise `Error: {:?}` on stderr (`main.rs:418-425`).

The storage layer already versions its own contract (`api::v1::API_VERSION`,
surfaced by `capabilities` and stamped into `export` snapshots). This spec adds
the **response** contract for CLI stdout, which is a separate consumer surface.

## Decisions

1. **Hard cut at 0.5.2.** All `--json` output becomes enveloped in one release;
   no opt-in transition flag. The primary consumer (the asobi agent skill) is
   updated in the same release.
2. **JSON Schema always ships.** `schemars` is a normal (non-optional)
   dependency and `asobi schema` is always present. A schema agents cannot rely
   on being there is pointless. Measured cost: +6 crates (`schemars`,
   `schemars_derive`, `serde_derive_internals`, `dyn-clone`, `ref-cast`,
   `ref-cast-impl`) — no C toolchain, derives reuse the existing `syn`/`quote`.

## The envelope

New module `src/response.rs`:

```rust
pub const RESPONSE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Response<T> {
    pub schema_version: u32,          // always RESPONSE_SCHEMA_VERSION
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,             // present iff ok == true
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>, // present iff ok == false
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResponseError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    Conflict,
    InvalidInput,
    Unsupported,
    Unavailable,
    Backend,
    Internal,
}
```

Success example (`asobi search "x" --json`):

```json
{
  "schemaVersion": 1,
  "ok": true,
  "data": { "entities": [ ... ], "relations": [ ... ] }
}
```

Error example (`asobi show missing --json`):

```json
{
  "schemaVersion": 1,
  "ok": false,
  "error": { "kind": "not_found", "message": "not found: missing" }
}
```

`data` carries the **existing** per-command shapes unchanged (`Graph`, history
array, stats object, receipts) — only the wrapper is new. No field renames
inside `data`.

## Emit helpers (single choke point)

In `src/response.rs`:

```rust
/// Serialize `data` in a success envelope to stdout.
pub fn emit<T: Serialize>(data: T) -> anyhow::Result<()>;
/// Serialize an error envelope to stdout (used on the --json path).
pub fn emit_err(err: &ResponseError) -> anyhow::Result<()>;
```

All command arms call `emit(...)`; the top-level error handler calls
`emit_err(...)` under `--json` and keeps the human `eprintln!` path otherwise.

## Error taxonomy

Map `api::v1::ApiError` (variants: `NotFound`, `Conflict`, `Unsupported`,
`Unavailable`, `Invalid`, `Backend`) at the `main.rs` boundary:

| ApiError       | ErrorKind      |
| -------------- | -------------- |
| `NotFound`     | `not_found`    |
| `Conflict`     | `conflict`     |
| `Invalid`      | `invalid_input`|
| `Unsupported`  | `unsupported`  |
| `Unavailable`  | `unavailable`  |
| `Backend`      | `backend`      |

Anything that surfaces as bare `anyhow` (not an `ApiError`) → `internal`.
Add `impl From<&ApiError> for ErrorKind` (or a match in the error handler);
downcast the `anyhow::Error` to `ApiError` at the boundary before falling back
to `internal`.

## `asobi schema` command

New subcommand, always available:

```
asobi schema                 # envelope schema + index of command data schemas
asobi schema --command show  # JSON Schema for the `show` response data shape
```

Derive `JsonSchema` on: `Response<T>`, `ResponseError`, `ErrorKind`, and every
`data` type — `model.rs` (`EntityOutput`, `Graph`, `RelationInput`,
`DetailedObservation`), `api::v1` (`Stats`, `ImportReport`, `BackupReceipt`,
`TruthVersion`, `BackendCapabilities`, `Snapshot`), plus any ad-hoc receipt
structs promoted out of `json!{}` (see Task 2). Generate with
`schemars::schema_for!`.

## Task breakdown (single PR)

The envelope and its schema are one atomic contract — releasing the enveloped
response without a discoverable schema is the half-measure this spec exists to
avoid. Ship all of it in one PR / one release (v0.5.2). Ordered so the tree
builds after each step:

1. Add `schemars` dep. Create `src/response.rs`: `Response<T>`, `ResponseError`,
   `ErrorKind`, `RESPONSE_SCHEMA_VERSION`, `emit`/`emit_err`; `#[derive(JsonSchema)]`
   throughout. Wire `mod response;` in lib.
2. Promote ad-hoc `json!{}` receipts (mutation `{deleted}`, stats json, etc.)
   into named `#[derive(Serialize, JsonSchema)]` structs so they have a schema.
   Derive `JsonSchema` on the existing `data` types in `model.rs` and `api::v1`.
3. Route every stdout-JSON site through `emit`. Known sites in `main.rs`:
   error handler (418-425), query (482), history (607), rm receipt (614-616),
   graph (687), search (710), show (724), stats json (772), capabilities
   (813-815), export (833-839), and the reset/import/backup/restore receipts.
   Grep `to_string_pretty` / `serde_json::json!` in `main.rs` for the full set.
4. Error taxonomy mapping (`ErrorKind` from `ApiError`) at the boundary; fix the
   stdout/stderr split so `--json` errors are always `ok:false` envelopes.
5. `asobi schema [--command <name>]` subcommand.
6. Extend `scripts/verify_cli.py`: assert every `--json` response is a valid
   envelope (`schemaVersion == 1`, `ok` present, `data` xor `error`), and
   validate each command's response against `asobi schema --command <name>`
   output (use a JSON Schema validator available via `uv`, e.g. `jsonschema`).
7. Update consumers/docs: `docs/usage.md`, `CLAUDE.md` "CLI" section,
   `model.rs` doc comment, and the asobi `SKILL.md` (agents now read
   `.data.entities` and branch on `.error.kind`). Add
   `docs/response-contract.md` describing the envelope + version policy.
8. `CHANGELOG.md` v0.5.2 with a **Breaking / Upgrade** section.

## Definition of done

- Every `--json` invocation returns `{schemaVersion, ok, data|error}`; `data`
  and `error` are mutually exclusive.
- `asobi schema` emits valid JSON Schema for the envelope and each command.
- `verify_cli.py` validates real responses against emitted schemas.
- `make check` and `make check-turso` pass.
- `SKILL.md` + `docs/usage.md` + `docs/response-contract.md` describe the
  enveloped contract and the version-bump policy (bump
  `RESPONSE_SCHEMA_VERSION` on any breaking `data`/envelope change).

## Version policy

`RESPONSE_SCHEMA_VERSION` starts at `1`. Additive fields (new optional keys) do
**not** bump it. Any removal, rename, or type change of an existing field, or an
envelope-shape change, bumps it. It is independent from `api::v1::API_VERSION`
(storage/export) — the two version different consumers on different cadences.

## Out of scope

- Human-readable output (plain `stats`, `init`/backup receipts printed as text)
  is unchanged; this only governs the machine/`--json` path.
- `export`/`import` keep their own `api_version` snapshot field — untouched.
- No opt-in/transition flag (hard cut, per decision 1).
