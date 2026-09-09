//! Version 2 of Asobi's backend-neutral core storage API.
//!
//! v2 is deliberately about the graph and its durable projections. It has no
//! document, embedding, vector, SQL, or filesystem-handle requirements.

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

pub const API_VERSION: u32 = 2;

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
    fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()>;
    fn add_observations(&self, observations: Vec<ObservationInput>, limit: usize) -> ApiResult<()>;
    fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()>;
    fn delete_entities(&self, names: Vec<String>) -> ApiResult<()>;
    fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()>;
    fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()>;
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()>;
    fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()>;
    fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()>;
    fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()>;
    fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()>;
    fn read_graph(&self) -> ApiResult<Graph>;
    fn read_graph_full(&self) -> ApiResult<Graph>;
    fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph>;
}

pub trait SearchStore {
    fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph>;
}

pub trait MaintenanceStore {
    fn stats(&self) -> ApiResult<Stats>;
    fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>>;
    fn purge(&self, request: PurgeRequest) -> ApiResult<PurgeReport>;
    fn reset(&self) -> ApiResult<()>;
    fn capabilities(&self) -> ApiResult<BackendCapabilities>;
    fn health(&self) -> ApiResult<BackendHealth>;
    fn location(&self) -> ApiResult<StorageLocation>;
}

pub trait TaskStore {
    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> ApiResult<Option<String>>;
    fn claim_next(&self, agent: &str) -> ApiResult<Option<String>>;
}
