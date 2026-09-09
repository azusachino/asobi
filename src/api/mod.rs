//! Versioned public storage API.
//!
//! The v2 capability traits are the current public contract. Breaking changes
//! belong in a new versioned module (`v3`, …); the unversioned re-exports below
//! are a migration convenience for current callers.

pub mod v2;

pub use v2::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo, GraphStore,
    MaintenanceStore, OpenNodes, PurgeCandidate, PurgeReport, PurgeRequest, SearchQuery,
    SearchStore, Stats, StorageLocation, TaskStore,
};
