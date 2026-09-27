//! Domain types, the async `api::v3` capability traits, and configuration and
//! path resolution. No I/O stack: nothing here needs a runtime or a driver.
//!
//! Implementation detail of the `asobi` and `asobi-storage` crates; the API
//! carries no stability promise beyond matching their version (ADR 0009).

pub mod api;
pub mod model;
pub mod normalize;
pub mod paths;
pub mod rpc;
