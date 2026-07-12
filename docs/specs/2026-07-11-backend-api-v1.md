# Asobi Backend API v1

Status: design baseline

The backend API is a public compatibility boundary. The CLI and domain layer
may depend on this API, but they must not depend on a database driver, SQL,
rows, pragmas, filesystem database handles, or driver-specific value types.

## Module and versioning

The Rust namespace is versioned:

    asobi::api::v1
    asobi::api::v2

v1 is frozen once the first alternate backend contract test passes. A breaking
change creates v2; it does not add a required method to v1. Unversioned
re-exports are compatibility conveniences only.

The API version is independent from the crate version and the on-disk schema
version. Snapshots carry both apiVersion and schemaVersion.

## Two-layer boundary

    CLI / domain services
              |
           api::v1
              |
      backend::turso / backend::postgres / future backends

The API layer owns domain models, validation, normalization, search requests,
capabilities, snapshots, and stable errors. A backend owns SQL or key-value
operations, schema creation, indexes, transactions, retries, connection
pools, and driver-specific errors.

SQL constants do not belong in the API layer or a shared constant.rs.
Backend-specific schema and queries live below the backend boundary.

## v1 capability traits

The final v1 surface is split by capability so a backend can advertise
unsupported optional features without leaking implementation details:

    GraphStore
      entity / observation / truth / relation CRUD
      graph reads, scoped reads, and name opening

    SearchStore
      graph keyword search
      topic keyword search
      stable SearchResult scoring contract

    DocumentStore
      topic and chunk persistence
      vector insert/delete/search

    SnapshotStore
      export/import a backend-neutral Snapshot
      no assumption that a backend is a local file

    MaintenanceStore
      stats, reset, health, and BackendCapabilities

Backend is the aggregate trait used by the application. The CLI receives an
API trait object or generic API implementation; it never receives a Turso,
LibSQL, PostgreSQL, or RocksDB connection.

## Backend compatibility probes

Each backend must pass the same contract suite:

1. graph CRUD and foreign-key-equivalent cleanup
2. immediate transaction atomicity and retry semantics
3. truth upsert and observation-limit behavior
4. exact-name fallback and keyword search result shape
5. snapshot round-trip
6. capability reporting for optional search/vector features

The first alternate probe is PostgreSQL-shaped and may use a test double
before a live PostgreSQL service is added. The purpose is to detect API
coupling, not to claim PostgreSQL support prematurely.

## PostgreSQL and RocksDB

PostgreSQL is a plausible future backend:

- relational graph tables and transactions map directly;
- FTS maps to tsvector plus a GIN index;
- vectors require optional pgvector;
- snapshots must be logical API snapshots, not file copies.

RocksDB is a different class of backend. It can implement the graph API, but
must provide its own relation indexes, truth indexes, FTS strategy, snapshot
format, and transaction/cleanup semantics. It must not be treated as a
drop-in SQL backend.
