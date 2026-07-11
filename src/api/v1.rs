//! Version 1 of Asobi's backend-neutral storage API.
//!
//! This layer is PURE DOMAIN: models, requests, results, capabilities,
//! snapshots, and a stable error type — and nothing else. It contains no SQL,
//! schema, indexes, driver types, rows, pragmas, or connection handles. Every
//! backend (turso, postgres, rocksdb) owns its schema and queries entirely
//! below this boundary and shares nothing but this contract. v1 freezes once
//! the first alternate backend passes the contract suite; breaking changes go
//! to v2.

// `async fn` in these public traits intentionally carries no `Send` bound: the
// storage is chosen once at startup and dispatched statically (see
// `crate::storage::Storage`),
// so the future's Send-ness is inferred at the call site rather than frozen into
// the contract. A `!Send` turso connection and a `Send` postgres pool both fit.
// Revisit (via `trait_variant`) only if a multi-threaded consumer needs it.
#![allow(async_fn_in_trait)]

pub const API_VERSION: u32 = 1;

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

// ---- Stable error surface -------------------------------------------------

/// Matchable errors at the API boundary. Backends map their driver failures
/// onto these variants; the driver's message is preserved in the payload, but
/// callers branch on the variant, never the string.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    /// An optional capability this backend does not implement (e.g. vectors on
    /// a backend without a vector index). Distinct from a real failure.
    #[error("unsupported by backend: {0}")]
    Unsupported(&'static str),
    #[error("backend unavailable: {0}")]
    Unavailable(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("backend error: {0}")]
    Backend(String),
}

pub type ApiResult<T> = std::result::Result<T, ApiError>;

// ---- Requests (named, not positional booleans) ----------------------------

#[derive(Debug, Clone, Default)]
pub struct OpenNodes {
    pub names: Vec<String>,
    pub with_ids: bool,
    pub expand: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub query: String,
    pub limit: usize,
    /// Truth filters, AND-combined (e.g. `status=READY`).
    pub filters: Vec<(String, String)>,
}

// ---- Value types ----------------------------------------------------------

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub entities: usize,
    pub relations: usize,
    pub observations: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilities {
    pub backend: String,
    /// True when the backend can serve the `search` command over graph content.
    /// Quality varies by backend: libSQL uses ranked, stemmed FTS5; the
    /// experimental Turso backend uses an unranked substring scan.
    pub keyword_search: bool,
    pub documents: bool,
    pub vectors: bool,
    pub logical_snapshots: bool,
    pub file_backup: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub score: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentChunk {
    pub id: String,
    pub topic_id: String,
    pub chunk_idx: u32,
    pub text: String,
    pub source: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSearchResult {
    pub id: String,
    pub topic_id: String,
    pub text: String,
    pub source: String,
    pub score: f32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicSnapshot {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub body: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealth {
    pub backend: String,
    pub reachable: bool,
    pub detail: Option<String>,
}

/// Identity and compatibility metadata for the selected backend.  The state
/// identifier is descriptive only; resolving its concrete path remains a
/// backend concern.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub backend: String,
    pub api_version: u32,
    pub schema_version: u32,
    pub state_id: String,
    pub capabilities: BackendCapabilities,
}

/// A persisted skill record.  Git/frontmatter parsing belongs to the
/// application layer; this type is only the storage-facing result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub entity_name: String,
    pub body: String,
    pub source: String,
    pub version: String,
    pub description: String,
}

/// Logical, backend-neutral graph snapshot used by export/import.  It is not
/// a physical database backup and must not contain driver-specific state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub api_version: u32,
    pub format_version: u32,
    pub source_backend: String,
    pub source_schema_version: u32,
    pub graph: Graph,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub entities_created: usize,
    pub entities_updated: usize,
    pub observations_added: usize,
    pub relations_added: usize,
    pub truths_updated: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub destination: std::path::PathBuf,
    pub keep: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReceipt {
    pub path: std::path::PathBuf,
    pub backend: String,
}

// ---- Capability traits ----------------------------------------------------

pub trait GraphStore {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()>;
    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> ApiResult<()>;
    async fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()>;
    async fn delete_entities(&self, names: Vec<String>) -> ApiResult<()>;
    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()>;
    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()>;
    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()>;
    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()>;
    async fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()>;
    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()>;
    async fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()>;

    /// Standard board read (truths + observation counts; no bodies).
    async fn read_graph(&self) -> ApiResult<Graph>;
    /// Full-detail read (every observation) — for a complete export.
    async fn read_graph_full(&self) -> ApiResult<Graph>;
    /// Full-detail subgraph for a scoped export bundle.
    async fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph>;
    async fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph>;
}

pub trait SearchStore {
    async fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph>;
}

pub trait DocumentStore {
    async fn upsert_topic(&self, topic: TopicSnapshot) -> ApiResult<()>;
    async fn delete_topic(&self, id: &str) -> ApiResult<()>;
    async fn insert_chunks(&self, chunks: Vec<DocumentChunk>) -> ApiResult<()>;
    async fn search_chunks(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> ApiResult<Vec<DocumentSearchResult>>;
    async fn delete_chunks_by_topic(&self, topic_id: &str) -> ApiResult<()>;
    async fn search_topics(&self, query: &str, limit: usize) -> ApiResult<Vec<SearchResult>>;
    async fn topics_by_id(&self, ids: &[String]) -> ApiResult<Vec<SearchResult>>;
}

/// Skill persistence only.  Installation orchestration, Git, and frontmatter
/// parsing remain in the application layer.
pub trait SkillStore {
    async fn list_skills(&self) -> ApiResult<Vec<SkillRecord>>;
    async fn skill_body(&self, entity_name: &str) -> ApiResult<Option<String>>;
    async fn upsert_skill(&self, skill: SkillRecord) -> ApiResult<()>;
    async fn remove_skills(&self, entity_names: Vec<String>) -> ApiResult<()>;
}

/// Logical export/import.  Implementations must preserve the snapshot format
/// and apply the graph atomically where their backend supports transactions.
pub trait SnapshotStore {
    async fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot>;
    async fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport>;
}

/// Optional physical backup capability.  A physical backup is not portable
/// across backends; callers must use `SnapshotStore` for handoff.
pub trait BackupStore {
    async fn backup(&self, request: BackupRequest) -> ApiResult<BackupReceipt>;
    async fn restore(&self, source: std::path::PathBuf, force: bool) -> ApiResult<()>;
}

/// Storage operation needed by compact's duplicate-topic report.  Markdown
/// rendering and file writes remain outside the backend boundary.
pub trait DocumentMaintenanceStore {
    async fn find_duplicate_clusters(&self, threshold: f32) -> ApiResult<Vec<Vec<String>>>;
}

pub trait MaintenanceStore {
    async fn stats(&self) -> ApiResult<Stats>;
    async fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>>;
    async fn reset(&self) -> ApiResult<()>;
    async fn capabilities(&self) -> ApiResult<BackendCapabilities>;
    async fn health(&self) -> ApiResult<BackendHealth>;
}
