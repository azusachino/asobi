//! Versioned public storage API.
//!
//! New breaking contracts must be introduced as a new versioned module
//! (`v2`, …). The unversioned re-exports below are a migration convenience for
//! current callers only.
//!
//! Backends are selected once at startup and dispatched statically. Prefer a
//! generic `<B: Backend>` or an `AnyBackend` enum over `dyn Backend` — the v1
//! traits use native `async fn` (no `async_trait`), which is not `dyn`-safe on
//! stable. TODO(codex): add `AnyBackend { Turso(..), Postgres(..) }` delegating
//! to each capability trait once the turso backend implements the full surface.

pub mod v1;

pub use v1::{
    ApiError, ApiResult, Backend, BackendCapabilities, BackendHealth, DocumentChunk,
    DocumentSearchResult, DocumentStore, GraphStore, MaintenanceStore, OpenNodes, SearchQuery,
    SearchResult, SearchStore, Snapshot, SnapshotStore, Stats, TopicSnapshot,
};
