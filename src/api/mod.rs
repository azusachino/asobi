//! Versioned public storage API.
//!
//! The v1 capability traits are the current public contract. Breaking changes
//! belong in a new versioned module (`v2`, …); the unversioned re-exports below
//! are only a migration convenience for current callers.

pub mod v1;
pub mod v2;

pub use v2::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo,
    BackupReceipt, BackupRequest, BackupStore, GraphStore, ImportReport, MaintenanceStore,
    SNAPSHOT_FORMAT_VERSION, SearchStore, SkillRecord, SkillStore, Snapshot, SnapshotStore, Stats,
    TaskStore,
};

pub use v1::{OpenNodes, SearchQuery, TruthVersion};
