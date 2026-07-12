# ADR: storage composition and naming

**Date:** 2026-07-11 **Status:** Accepted **Epic:** `asobi:backend-boundary-v1`

## Decision

Asobi separates the stable storage contract, application use cases, concrete storage implementations, and the runtime composition root.

```text
CLI -> AsobiRuntime -> application services -> api::v1 contracts -> Storage
                                                        └── LibsqlStore | TursoStore
```

`main.rs` constructs `AsobiRuntime` and calls application use cases. It does not import a concrete store, a driver type, SQL, schema constants, or a backend-specific filename.

## Naming

- `GraphStore`, `SearchStore`, `DocumentStore`, and `MaintenanceStore` are narrow storage capabilities in `api::v1`.
- `LibsqlStore` and `TursoStore` are concrete provider adapters under `backend/`.
- `Storage` is the selected-store composite that delegates the capability traits to the selected provider.
- `AsobiRuntime` is the application composition root. It owns workspace configuration, the selected `Storage`, and application services.
- `ExportBundle` is application-level transfer data. It is not a backend snapshot and does not require a `SnapshotStore` trait.
- `PhysicalBackup` is an optional storage capability because file backups are engine-specific.

The old `Backend` aggregate and `ApplicationBackend` mega-trait are not used. Application services depend on the narrowest capability they need. Rust trait default methods may provide backend-neutral composition, but a default must not promise atomicity or a storage feature that the provider cannot guarantee.

## Composition model

`Storage` is an enum in the backend layer. Each capability implementation delegates to the matching provider. The default factory constructs `Storage::Libsql`; the Turso variant is compiled and selectable only when its experimental feature is enabled. State-file resolution remains private to each provider, so the application never knows whether the selected state is `asobi.db` or `asobi.turso.db`.

Application services are ordinary structs or modules, not storage traits. For example, export/import reads and writes through `GraphStore`, while skill frontmatter parsing and Git operations remain application code.

## Tokio influence

Tokio's runtime exposes a small public entry point and a builder/composition root while keeping scheduler, driver, and implementation modules internal. Its builder selects a concrete runtime and then the public runtime handle is used by the application. Asobi follows the same shape: select and compose once in `AsobiRuntime`, then keep command code on stable capability APIs.

Reference: <https://docs.rs/tokio/latest/tokio/runtime/>

## Consequences

Positive:

- frontend code is independent of libSQL and Turso;
- optional capabilities remain explicit instead of inflating one aggregate trait;
- the provider owns state-file identity and database lifecycle;
- a second backend can be tested through the same capability contract;
- application behavior can be tested with in-memory/fake capability adapters.

Trade-offs:

- `Storage` needs delegation code for each capability;
- application services must declare capability bounds explicitly;
- physical backup remains intentionally distinct from portable export/import.

## Rejected alternatives

- A single `ApplicationBackend` trait containing every operation: forces unrelated optional capabilities onto every provider.
- Aliasing Turso names to libSQL names: hides backend identity and risks opening the wrong state file.
- Putting Git, Markdown, or CLI workflows in `api::v1`: makes the storage contract application-specific and difficult to implement elsewhere.
