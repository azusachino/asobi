//! Versioned public storage API.
//!
//! The v1 capability traits are the current public contract. Breaking changes
//! belong in a new versioned module (`v2`, …); the unversioned re-exports below
//! are only a migration convenience for current callers.

pub mod v1;

pub use v1::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo,
    BackupReceipt, BackupRequest, BackupStore, DocumentChunk, DocumentMaintenanceStore,
    DocumentSearchResult, DocumentStore, GraphStore, ImportReport, MaintenanceStore, OpenNodes,
    SNAPSHOT_FORMAT_VERSION, SearchQuery, SearchResult, SearchStore, SkillRecord, SkillStore,
    Snapshot, SnapshotStore, Stats, TopicSnapshot,
};
