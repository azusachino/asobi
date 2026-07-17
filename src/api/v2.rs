//! Version 2 of Asobi's backend-neutral core storage API.
//!
//! v2 is deliberately about the graph and its durable projections. It has no
//! document, embedding, vector, SQL, or filesystem-handle requirements.

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

pub const API_VERSION: u32 = 2;
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub query: String,
    pub limit: usize,
    pub filters: Vec<(String, String)>,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthVersion {
    pub key: String,
    pub value: String,
    pub valid_from: String,
    pub valid_until: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRecord {
    pub entity_name: String,
    pub body: String,
    pub source: String,
    pub version: String,
    pub description: String,
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
    pub entity_types: Vec<String>,
    pub statuses: Vec<String>,
    pub older_than_days: u32,
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
    pub logical_snapshots: bool,
    pub physical_backup: bool,
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

#[derive(Debug, Clone, Default, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub entities_created: usize,
    pub entities_updated: usize,
    pub observations_added: usize,
    pub relations_added: usize,
    pub truths_updated: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub api_version: u32,
    pub format_version: u32,
    pub source_backend: String,
    pub source_schema_version: u32,
    pub graph: Graph,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRequest {
    pub destination: std::path::PathBuf,
    pub keep: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupReceipt {
    pub path: std::path::PathBuf,
    pub backend: String,
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
    fn truth_history(&self, entity: &str, key: Option<&str>) -> ApiResult<Vec<TruthVersion>>;
    fn read_graph(&self) -> ApiResult<Graph>;
    fn read_graph_full(&self) -> ApiResult<Graph>;
    fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph>;
    fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph>;
}

pub trait SearchStore {
    fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph>;
}

pub trait SkillStore {
    fn list_skills(&self) -> ApiResult<Vec<SkillRecord>>;
    fn skill_body(&self, entity_name: &str) -> ApiResult<Option<String>>;
    fn upsert_skill(&self, skill: SkillRecord) -> ApiResult<()>;
    fn remove_skills(&self, entity_names: Vec<String>) -> ApiResult<()>;
}

pub trait SnapshotStore {
    fn export_snapshot(&self, scope: &[String], rationale: bool) -> ApiResult<Snapshot>;
    fn import_snapshot(&self, snapshot: Snapshot) -> ApiResult<ImportReport>;
}

pub trait BackupStore {
    fn backup(&self, request: BackupRequest) -> ApiResult<BackupReceipt>;
    fn restore(self, source: std::path::PathBuf, force: bool) -> ApiResult<()>
    where
        Self: Sized;
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
