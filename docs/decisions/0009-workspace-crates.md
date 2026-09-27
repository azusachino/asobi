---
id: 0009
title: "0009. A workspace of four crates, a separate server binary, a thin CLI"
date: 2026-09-27
status: accepted
tags: [structure, crates, release, v0.8]
related: [0005-remote-server.md, 0008-async-storage-on-sqlx.md]
---

## Context

Asobi is one crate, `asobi`, producing one binary. 0.8 adds a server ([0005](0005-remote-server.md)) built on tokio, axum and hyper, and a remote client built on reqwest, while storage moves to sqlx ([0008](0008-async-storage-on-sqlx.md)).

Most people who install Asobi use it locally and never run or reach a server. In one crate, every one of them would compile and ship the server stack and the HTTP client. The storage boundary is also only enforced by a script (`scripts/verify_storage_boundary.py`) that greps for driver names.

## Decision

### Four crates in one workspace

| Crate | Holds | Depends on |
| --- | --- | --- |
| `asobi-core` | domain types, the async `api::v3` traits and `ApiError`, the HTTP operation contract (names, request types, status/kind table), configuration and path resolution | serde, toml; no I/O stack |
| `asobi-storage` | `SqliteStore` on sqlx, migrations, the sweep | `asobi-core`, sqlx |
| `asobi` | the CLI binary `asobi`: commands, tasks, compact, local/remote selection, and `RemoteStore` behind the `remote` feature | `asobi-core`, `asobi-storage`, tokio (current-thread); reqwest only with `remote` |
| `asobi-server` | the server binary `asobi-server`: routes, graph registry, background sweep | `asobi-core`, `asobi-storage`, tokio (multi-thread), axum, hyper |

The storage boundary becomes structural: only `asobi-storage` depends on sqlx, so no other crate can reach a driver type. `scripts/verify_storage_boundary.py` is deleted.

### The server is its own binary

`asobi serve` becomes the `asobi-server` binary. The CLI never links the server stack, and the container image ([0005](0005-remote-server.md)) needs only this binary.

### The CLI stays thin: remote mode is a feature

The `asobi` crate's remote client sits behind a cargo feature, `remote`, **off by default**. `cargo install asobi` builds a local-only CLI with no HTTP stack; `cargo install asobi --features remote` adds remote mode. The prebuilt release binaries are built with `remote` enabled. A build without the feature that finds `remote` configured fails with a clear message ("this asobi was built without remote support") rather than silently using the local graph.

### All crates are published together

Every release publishes all four crates to crates.io at the same version, so `cargo install asobi` and `cargo install asobi-server` keep working. `asobi-core` and `asobi-storage` are implementation details; their READMEs say so, and their APIs carry no stability promise beyond matching the other crates' version.

## Consequences

- One version number for the workspace; the release workflow publishes the crates in dependency order and checks that the tag matches it.
- Local-only users compile neither axum/hyper nor reqwest.
- Remote-mode users must install with the feature or use the prebuilt binary; the error message tells a user who configured `remote` on a local-only build exactly that.
- Tests move with their code: storage contract and sweep tests to `asobi-storage`, CLI tests to `asobi`, server tests to `asobi-server`, end-to-end tests to `asobi-server` (which can start a server and drive the CLI binary).
