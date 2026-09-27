//! The bundled SQLite provider behind the async `api::v3` traits. This crate
//! is the only one allowed to depend on a storage driver (ADR 0009) -- the
//! boundary Cargo enforces structurally.
//!
//! Implementation detail of the `asobi` crate; no stability promise beyond
//! matching its version.

pub mod storage;

pub use storage::{DEFAULT_ABANDON_DAYS, DEFAULT_RETENTION_DAYS, SqliteStore};

/// The selected storage provider.
pub type Storage = SqliteStore;
