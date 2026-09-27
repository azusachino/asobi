//! The Asobi graph server (ADR 0005, ADR 0009): named graphs under a data
//! directory, served over the HTTP protocol in `asobi_core::protocol`.
//!
//! - [`GraphRegistry`]: graph-name validation, lazily opened stores, sweeps.
//! - [`App`]/[`BoundServer`]: the axum router and the bound server.
//!
//! Tests start a [`BoundServer`] on `127.0.0.1:0` and speak real HTTP.
//!
//! The server **never** shares the CLI's data directory: `--data-dir` is a
//! required argument with no asobi.toml/XDG fallback, so a server and a
//! local CLI on one host cannot silently open the same graph file (and the
//! hourly sweep cannot walk the CLI's directory).

pub mod args;
pub mod registry;
mod server;
mod sweep;

pub use args::parse_args;
pub use registry::GraphRegistry;
pub use server::{App, BoundServer, run};

/// The interval the production background sweep runs on.
pub const SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60 * 60);
