# Design Spec: Async Rust Masterclass (rosemary)

**Topic**: Comprehensive async Rust patterns, observability, networking, and system design.
**Status**: DRAFT (Review required)
**Proposed By**: Gemini CLI

## Overview

The `rosemary` project will evolve into a "best practices" async Rust learning repository. This spec covers four major areas: Observability, Advanced Networking, Data Persistence, and System Patterns.

## Goals

1.  **Observability-First**: Every async example should be observable (logs/metrics) and reliable (graceful shutdown).
2.  **Protocol Diversity**: Show how to build systems using gRPC, WebSockets, and custom framed protocols.
3.  **Modern Persistence**: Demonstrate async-first database and caching layers.
4.  **Complex Behaviors**: Implement scheduling, actor patterns, and stream processing without external frameworks.

## Architecture

The project will transition from a flat `examples/` collection to a more structured hybrid:

- `src/lib.rs`: Shared utilities (observability, shutdown, common error types).
- `src/observability/`: Tracing and metrics configuration.
- `src/shutdown/`: Graceful shutdown signal handling.
- `src/protocols/`: Shared proto definitions and codecs.
- `examples/`: Specialized standalone examples demonstrating each phase.

---

## Phase 1: Observability & Reliability

### Features

- **Tracing**: `tracing` + `tracing-subscriber` (with `env-filter`).
- **Metrics**: `metrics` + `metrics-exporter-prometheus` (exposing `/metrics`).
- **Shutdown**: `tokio-util::sync::CancellationToken` for centralized cancellation.

### Architecture

- `src/observability.rs`: `init_tracing()` and `init_metrics()` functions.
- `src/shutdown.rs`: `GracefulShutdown` struct that listens for `SIGINT`/`SIGTERM` and triggers a cancellation token.

---

## Phase 2: Advanced Networking

### Features

- **gRPC**: `tonic` + `prost` for high-performance service communication.
- **WebSockets**: `tokio-tungstenite` for bidirectional streaming.
- **Framing**: `tokio-util` `Codec`/`Framed` for custom binary protocols.

### Components

- `proto/`: `.proto` files for gRPC definitions.
- `examples/grpc_server.rs`: Sample gRPC service.
- `examples/websocket_chat.rs`: Multi-client async chat.
- `examples/framed_protocol.rs`: Custom binary length-delimited communication.

---

## Phase 3: Data & Storage

### Features

- **SQLx**: Async SQL with compile-time query verification.
- **Redis**: Async caching and pub/sub via `fred`.

### Components

- `examples/sqlx_db.rs`: CRUD with SQLite/Postgres.
- `examples/redis_cache.rs`: Caching layer with TTL and invalidation logic.

---

## Phase 4: System Patterns

### Features

- **Scheduling**: `tokio-cron-scheduler` for periodic tasks.
- **Actor Pattern**: "Pure Tokio" actors using channels and background loop tasks.
- **Stream Processing**: Aggregation and windowing using `futures` and `tokio-stream`.

### Components

- `examples/task_scheduler.rs`: Cron-style job runner.
- `examples/tokio_actor.rs`: Handle/Command/Loop pattern for state isolation.
- `examples/stream_aggregate.rs`: Windowed stream processing demo.

---

## Implementation Strategy

1.  **Branching**: Work on `feat/async-masterclass`.
2.  **Sequencing**: Phase 1 -> Phase 2 -> Phase 3 -> Phase 4.
3.  **Verification**: Each phase must pass `make check` and have at least one runnable example in `examples/`.

## Self-Review

- **Placeholder scan**: No TBDs. All primary crates (`tonic`, `sqlx`, `tracing`) are selected.
- **Internal consistency**: Every phase builds on Phase 1's observability.
- **Scope check**: Large, but decomposed into 4 logical phases for incremental implementation.
- **Ambiguity check**: Protocol selection is explicit (`tonic`, `tokio-tungstenite`).
