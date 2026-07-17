//! Versioned public storage API.
//!
//! The v2 capability traits are the current public contract. Breaking changes
//! belong in a new versioned module (`v3`, …); the unversioned re-exports below
//! are a migration convenience for current callers.

pub mod v2;

pub use v2::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo,
    BackupReceipt, BackupRequest, BackupStore, GraphStore, ImportReport, MaintenanceStore,
    OpenNodes, SNAPSHOT_FORMAT_VERSION, SearchQuery, SearchStore, SkillRecord, SkillStore,
    Snapshot, SnapshotStore, Stats, TaskStore, TruthVersion,
};
