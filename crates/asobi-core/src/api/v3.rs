//! Version 3 of Asobi's backend-neutral core storage API: `v2` made async on
//! the SQL driver (ADR 0008). Same four capabilities, same types, Send futures.
//!
//! v2 was deliberately about the graph and its durable projections. It has no
//! document, embedding, vector, SQL, or filesystem-handle requirements.

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};
use std::future::Future;

pub const API_VERSION: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
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

#[derive(Debug, Clone, Default)]
pub struct OpenNodes {
    pub names: Vec<String>,
    pub with_ids: bool,
    pub expand: Vec<String>,
    /// How many of the most recent observations to return per entity; 0 for all.
    ///
    /// `show` is the only eager read, and it was unbounded — loading a session
    /// entity at the 200-observation cap cost tens of thousands of tokens to
    /// answer "where was I". Context is the scarce resource, so the default is
    /// the current end of the trail, with `observationCount` still reporting
    /// the true total.
    pub observation_limit: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub query: String,
    pub limit: usize,
    pub filters: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub entities: usize,
    pub relations: usize,
    pub observations: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeRequest {
    /// How many days a finished session or task survives.
    pub older_than_days: u32,
    /// Delete rather than preview.
    pub apply: bool,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeCandidate {
    pub name: String,
    pub entity_type: String,
    pub status: String,
    pub last_activity: String,
    pub observations: usize,
    pub relations: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurgeReport {
    pub dry_run: bool,
    pub older_than_days: u32,
    pub candidates: Vec<PurgeCandidate>,
    pub deleted: usize,
}

/// Backend capabilities describe behavior that callers may adapt to. They do
/// not expose a driver, SQL dialect, or storage layout.
#[derive(Debug, Clone, Default, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendCapabilities {
    pub backend: String,
    pub keyword_search: bool,
    /// `fts5`, `indexed-token`, or `none`.
    pub keyword_search_kind: String,
    /// Whether separate CLI processes may open the same state concurrently.
    pub multi_process: bool,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocation {
    pub database_path: String,
    pub journal_mode: String,
    pub schema_version: u32,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendHealth {
    pub backend: String,
    pub reachable: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub backend: String,
    pub api_version: u32,
    pub schema_version: u32,
    pub state_id: String,
    pub capabilities: BackendCapabilities,
}

pub trait GraphStore {
    fn create_entities(
        &self,
        entities: Vec<EntityInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn create_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn delete_entities(&self, names: Vec<String>) -> impl Future<Output = ApiResult<()>> + Send;
    fn delete_observations(
        &self,
        deletions: Vec<ObservationDeletion>,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn delete_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn delete_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn truth_upsert(
        &self,
        entity: &str,
        key: &str,
        value: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send;
    fn truth_delete(&self, entity: &str, key: &str) -> impl Future<Output = ApiResult<()>> + Send;
    fn read_graph(&self) -> impl Future<Output = ApiResult<Graph>> + Send;
    fn read_graph_full(&self) -> impl Future<Output = ApiResult<Graph>> + Send;
    fn open_nodes(&self, req: OpenNodes) -> impl Future<Output = ApiResult<Graph>> + Send;
}
pub trait SearchStore {
    fn search_nodes(&self, query: SearchQuery) -> impl Future<Output = ApiResult<Graph>> + Send;
}
pub trait MaintenanceStore {
    fn stats(&self) -> impl Future<Output = ApiResult<Stats>> + Send;
    fn stats_per_entity(&self) -> impl Future<Output = ApiResult<Vec<(String, usize)>>> + Send;
    fn purge(&self, request: PurgeRequest) -> impl Future<Output = ApiResult<PurgeReport>> + Send;
    fn reset(&self) -> impl Future<Output = ApiResult<()>> + Send;
    fn capabilities(&self) -> impl Future<Output = ApiResult<BackendCapabilities>> + Send;
    fn health(&self) -> impl Future<Output = ApiResult<BackendHealth>> + Send;
    fn location(&self) -> impl Future<Output = ApiResult<StorageLocation>> + Send;
}
pub trait TaskStore {
    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> impl Future<Output = ApiResult<Option<String>>> + Send;
    fn claim_next(&self, agent: &str) -> impl Future<Output = ApiResult<Option<String>>> + Send;
}

//
// Send futures: the axum server (WP4) runs on a multi-threaded tokio runtime,
// so every future that crosses an `.await` in a handler must be `Send`. The
// traits declare `-> impl Future<...> + Send` (an RPITIT that captures
// `&self`), and implementations use `async fn` — the driver's futures are
// Send, so the bound holds without `async_trait` boxing.
