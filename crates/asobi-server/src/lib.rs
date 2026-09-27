//! The Asobi graph server (ADR 0005, ADR 0009): named graphs under a data
//! directory, served over the HTTP protocol in `asobi_core::protocol`.
//!
//! - [`GraphRegistry`]: graph-name validation, lazily opened stores, sweeps.
//! - [`App`]/[`BoundServer`]: the axum router and the bound server.
//!
//! Tests start a [`BoundServer`] on `127.0.0.1:0` and speak real HTTP.

pub mod registry;
mod server;
mod sweep;

pub use registry::GraphRegistry;
pub use server::{App, BoundServer, run};

/// The interval the production background sweep runs on.
pub const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);
