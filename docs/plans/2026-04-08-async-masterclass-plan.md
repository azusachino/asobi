# Async Rust Masterclass Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transform `rosemary` into a best-practices async Rust repository covering observability, networking, storage, and system patterns.

**Architecture:** Hybrid library/examples structure. Shared infrastructure in `src/` (observability, shutdown, shared protocols) and standalone, runnable demos in `examples/`.

**Tech Stack:** Rust 2024, Tokio (full), Tracing, Metrics, Tonic (gRPC), SQLx, Redis (Fred), Tokio-Util.

---

## Phase 1: Observability & Reliability

### Task 1.1: Dependencies & Core Infrastructure

**Files:**

- Modify: `Cargo.toml`
- Create: `src/observability.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add observability and utility dependencies**

```toml
# Add to Cargo.toml [dependencies]
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
metrics = "0.23"
metrics-exporter-prometheus = "0.15"
tokio-util = { version = "0.7", features = ["full"] }
```

- [ ] **Step 2: Create the observability module**

```rust
// src/observability.rs
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use metrics_exporter_prometheus::PrometheusBuilder;

pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();
}

pub fn init_metrics() {
    let builder = PrometheusBuilder::new();
    builder.install().expect("failed to install Prometheus recorder");
}
```

- [ ] **Step 3: Export the new module in lib.rs**

```rust
// src/lib.rs
pub mod observability;
// ... (rest of lib.rs)
```

- [ ] **Step 4: Verify compilation**
      Run: `make check`
      Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/observability.rs src/lib.rs
git commit -m "feat: init observability (tracing + metrics)"
```

### Task 1.2: Graceful Shutdown Pattern

**Files:**

- Create: `src/shutdown.rs`
- Modify: `src/lib.rs`
- Create: `examples/graceful_shutdown.rs`

- [ ] **Step 1: Implement the Shutdown Manager**

```rust
// src/shutdown.rs
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct GracefulShutdown {
    token: CancellationToken,
}

impl GracefulShutdown {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub async fn wait_for_signal(self) {
        let ctrl_c = async {
            signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => info!("received Ctrl+C signal"),
            _ = terminate => info!("received terminate signal"),
        }

        self.token.cancel();
    }
}
```

- [ ] **Step 2: Add example demonstrating the pattern**

```rust
// examples/graceful_shutdown.rs
use rosemary::observability::init_tracing;
use rosemary::shutdown::GracefulShutdown;
use tracing::info;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let shutdown = GracefulShutdown::new();
    let token = shutdown.token();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("worker: received cancellation, shutting down...");
                    break;
                }
                _ = sleep(Duration::from_secs(1)) => {
                    info!("worker: doing work...");
                }
            }
        }
    });

    shutdown.wait_for_signal().await;
    info!("main: signal received, waiting for final cleanup...");
    sleep(Duration::from_secs(1)).await;
    info!("main: goodbye!");
    Ok(())
}
```

- [ ] **Step 3: Verify the example**
      Run: `make run-examples EXAMPLE=graceful_shutdown`
      (Press Ctrl+C to trigger shutdown)
      Expected: Logs show "received Ctrl+C signal", "worker: shutting down", and "main: goodbye!"

- [ ] **Step 4: Commit**

```bash
git add src/shutdown.rs examples/graceful_shutdown.rs
git commit -m "feat: implement graceful shutdown pattern"
```

---

## Phase 2: Advanced Networking

### Task 2.1: gRPC with Tonic

**Files:**

- Modify: `Cargo.toml`
- Create: `proto/hello.proto`
- Create: `build.rs`
- Create: `examples/grpc_server.rs`
- Create: `examples/grpc_client.rs`

- [ ] **Step 1: Add Tonic dependencies**

```toml
# Cargo.toml [dependencies]
tonic = "0.12"
prost = "0.13"

[build-dependencies]
tonic-build = "0.12"
```

- [ ] **Step 2: Define Protobuf service**

```proto
// proto/hello.proto
syntax = "proto3";
package hello;

service Greeter {
  rpc SayHello (HelloRequest) returns (HelloResponse);
}

message HelloRequest {
  string name = 1;
}

message HelloResponse {
  string message = 1;
}
```

- [ ] **Step 3: Create build.rs for Protobuf compilation**

```rust
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::compile_protos("proto/hello.proto")?;
    Ok(())
}
```

- [ ] **Step 4: Implement gRPC Server Example**
      (See full code in docs/plans/grpc_server_code.rs for detail)

- [ ] **Step 5: Verify gRPC interaction**
      Run: `make run-examples EXAMPLE=grpc_server`
      Run: `make run-examples EXAMPLE=grpc_client` (in separate terminal)
      Expected: Client receives "Hello <name>!"

- [ ] **Step 6: Commit**

```bash
git add proto/ build.rs examples/grpc_server.rs examples/grpc_client.rs
git commit -m "feat: add gRPC example using tonic"
```

---

## Phase 3: Data & Storage

### Task 3.1: Async DB with SQLx

**Files:**

- Modify: `Cargo.toml`
- Create: `examples/sqlx_sqlite.rs`

- [ ] **Step 1: Add SQLx dependencies**

```toml
# Cargo.toml [dependencies]
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "macros"] }
```

- [ ] **Step 2: Implement SQLite Example**
      (See code in examples/sqlx_sqlite.rs)

- [ ] **Step 3: Commit**

```bash
git add examples/sqlx_sqlite.rs
git commit -m "feat: add SQLx SQLite example"
```

---

## Phase 4: System Patterns

### Task 4.1: Tokio Actor Pattern

**Files:**

- Create: `examples/tokio_actor.rs`

- [ ] **Step 1: Implement the Handle/Command/Loop pattern**

```rust
// examples/tokio_actor.rs
// ... actor implementation details ...
```

- [ ] **Step 2: Commit**

```bash
git add examples/tokio_actor.rs
git commit -m "feat: implement tokio actor pattern example"
```
