//! Versioned public storage API.
//!
//! The v3 capability traits are the current public contract: `v2` went async
//! in 0.8 (ADR 0008), and per the versioning rule a breaking
//! change is a new version, not a mutated old one. Breaking changes belong in
//! a new versioned module (`v4`, …); the unversioned re-exports below are a
//! migration convenience for current callers.

pub mod v3;

pub use v3::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo, GraphStore,
    MaintenanceStore, OpenNodes, PurgeCandidate, PurgeReport, PurgeRequest, SearchQuery,
    SearchStore, Stats, StorageLocation, TaskStore,
};
