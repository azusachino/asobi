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
// backend is chosen once at startup and dispatched statically (see `AnyBackend`),
// so the future's Send-ness is inferred at the call site rather than frozen into
// the contract. A `!Send` turso connection and a `Send` postgres pool both fit.
// Revisit (via `trait_variant`) only if a multi-threaded consumer needs it.
#![allow(async_fn_in_trait)]

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

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

/// Backend-neutral logical snapshot — never a file copy. This is the ONLY
/// cross-backend interchange format, so turso -> postgres -> rocksdb round-trips
/// through it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub api_version: u32,
    pub schema_version: u32,
    pub graph: Graph,
    pub topics: Vec<TopicSnapshot>,
    pub chunks: Vec<DocumentChunk>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealth {
    pub backend: String,
    pub reachable: bool,
    pub detail: Option<String>,
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
    async fn search_topics(&self, query: &str, limit: usize) -> ApiResult<Vec<SearchResult>>;
    async fn topics_by_id(&self, ids: &[String]) -> ApiResult<Vec<SearchResult>>;
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
}

pub trait SnapshotStore {
    async fn export_snapshot(&self) -> ApiResult<Snapshot>;
    async fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<()>;
}

pub trait MaintenanceStore {
    async fn stats(&self) -> ApiResult<Stats>;
    async fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>>;
    async fn reset(&self) -> ApiResult<()>;
    async fn capabilities(&self) -> ApiResult<BackendCapabilities>;
    async fn health(&self) -> ApiResult<BackendHealth>;
}

/// The aggregate an application depends on. A backend that satisfies every
/// capability trait is automatically a `Backend`.
pub trait Backend:
    GraphStore + SearchStore + DocumentStore + SnapshotStore + MaintenanceStore
{
}

impl<T> Backend for T where
    T: GraphStore + SearchStore + DocumentStore + SnapshotStore + MaintenanceStore
{
}
