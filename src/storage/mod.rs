//! Concrete storage providers.

pub mod libsql;
#[cfg(feature = "turso-experimental")]
pub mod turso;

pub use libsql::LibsqlStore;
#[cfg(feature = "turso-experimental")]
pub use turso::TursoStore;

use crate::api::v1::{
    ApiResult, BackendCapabilities, BackendHealth, BackupReceipt, BackupRequest, BackupStore,
    DocumentChunk, DocumentMaintenanceStore, DocumentSearchResult, DocumentStore, GraphStore,
    MaintenanceStore, OpenNodes, SearchQuery, SearchResult, SearchStore, SkillRecord, SkillStore,
    Stats, TopicSnapshot,
};
use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

/// The selected storage provider.  Provider selection happens once at startup;
/// application services use this stable composite and never match on a driver.
///
/// libSQL is the default provider. The Turso variant is compiled and selectable
/// only when the experimental `turso-experimental` feature is enabled.
pub enum Storage {
    Libsql(Box<LibsqlStore>),
    #[cfg(feature = "turso-experimental")]
    Turso(Box<TursoStore>),
}

impl Storage {
    /// Open the selected provider. libSQL is the default; a build compiled with
    /// `turso-experimental` additionally honors `ASOBI_BACKEND=turso` to select
    /// the experimental Turso provider. Selection happens here — the only place
    /// that maps a chosen backend to a concrete store — so command code stays on
    /// the stable capability APIs.
    pub async fn open_default() -> crate::Result<Self> {
        #[cfg(feature = "turso-experimental")]
        if std::env::var(crate::paths::ENV_BACKEND)
            .is_ok_and(|value| value.eq_ignore_ascii_case("turso"))
        {
            return Ok(Self::Turso(Box::new(TursoStore::open().await?)));
        }
        Ok(Self::Libsql(Box::new(LibsqlStore::open().await?)))
    }

    pub fn from_libsql(store: LibsqlStore) -> Self {
        Self::Libsql(Box::new(store))
    }

    #[cfg(feature = "turso-experimental")]
    pub fn from_turso(store: TursoStore) -> Self {
        Self::Turso(Box::new(store))
    }
}

impl SkillStore for Storage {
    async fn list_skills(&self) -> ApiResult<Vec<SkillRecord>> {
        match self {
            Self::Libsql(store) => store.list_skills().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.list_skills().await,
        }
    }

    async fn skill_body(&self, entity_name: &str) -> ApiResult<Option<String>> {
        match self {
            Self::Libsql(store) => store.skill_body(entity_name).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.skill_body(entity_name).await,
        }
    }

    async fn upsert_skill(&self, skill: SkillRecord) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.upsert_skill(skill).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.upsert_skill(skill).await,
        }
    }

    async fn remove_skills(&self, entity_names: Vec<String>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.remove_skills(entity_names).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.remove_skills(entity_names).await,
        }
    }
}

impl BackupStore for Storage {
    async fn backup(&self, _request: BackupRequest) -> ApiResult<BackupReceipt> {
        Err(crate::api::v1::ApiError::Unsupported("physical backup"))
    }

    async fn restore(&self, _source: std::path::PathBuf, _force: bool) -> ApiResult<()> {
        Err(crate::api::v1::ApiError::Unsupported("physical restore"))
    }
}

impl DocumentMaintenanceStore for Storage {
    async fn find_duplicate_clusters(&self, threshold: f32) -> ApiResult<Vec<Vec<String>>> {
        match self {
            Self::Libsql(store) => store.find_duplicate_clusters(threshold).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.find_duplicate_clusters(threshold).await,
        }
    }
}

impl GraphStore for Storage {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.create_entities(entities).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.create_entities(entities).await,
        }
    }

    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.add_observations(observations, limit).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.add_observations(observations, limit).await,
        }
    }

    async fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.create_relations(relations).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.create_relations(relations).await,
        }
    }

    async fn delete_entities(&self, names: Vec<String>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_entities(names).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_entities(names).await,
        }
    }

    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_observations(deletions).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_observations(deletions).await,
        }
    }

    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_observation_by_id(entity_name, id).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_observation_by_id(entity_name, id).await,
        }
    }

    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => {
                store
                    .update_observation_by_id(entity_name, id, new_content)
                    .await
            }
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => {
                store
                    .update_observation_by_id(entity_name, id, new_content)
                    .await
            }
        }
    }

    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => {
                store
                    .update_observation(entity_name, old_content, new_content)
                    .await
            }
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => {
                store
                    .update_observation(entity_name, old_content, new_content)
                    .await
            }
        }
    }

    async fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_relations(relations).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_relations(relations).await,
        }
    }

    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.truth_upsert(entity, key, value).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.truth_upsert(entity, key, value).await,
        }
    }

    async fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.truth_delete(entity, key).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.truth_delete(entity, key).await,
        }
    }

    async fn read_graph(&self) -> ApiResult<Graph> {
        match self {
            Self::Libsql(store) => store.read_graph().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.read_graph().await,
        }
    }

    async fn read_graph_full(&self) -> ApiResult<Graph> {
        match self {
            Self::Libsql(store) => store.read_graph_full().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.read_graph_full().await,
        }
    }

    async fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph> {
        match self {
            Self::Libsql(store) => store.read_graph_scoped(scope, rationale).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.read_graph_scoped(scope, rationale).await,
        }
    }

    async fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph> {
        match self {
            Self::Libsql(store) => store.open_nodes(req).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.open_nodes(req).await,
        }
    }
}

impl SearchStore for Storage {
    async fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph> {
        match self {
            Self::Libsql(store) => store.search_nodes(query).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.search_nodes(query).await,
        }
    }
}

impl MaintenanceStore for Storage {
    async fn stats(&self) -> ApiResult<Stats> {
        match self {
            Self::Libsql(store) => store.stats().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.stats().await,
        }
    }

    async fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>> {
        match self {
            Self::Libsql(store) => store.stats_per_entity().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.stats_per_entity().await,
        }
    }

    async fn reset(&self) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.reset().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.reset().await,
        }
    }

    async fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        match self {
            Self::Libsql(store) => store.capabilities().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.capabilities().await,
        }
    }

    async fn health(&self) -> ApiResult<BackendHealth> {
        match self {
            Self::Libsql(store) => store.health().await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.health().await,
        }
    }
}

impl DocumentStore for Storage {
    async fn upsert_topic(&self, topic: TopicSnapshot) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.upsert_topic(topic).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.upsert_topic(topic).await,
        }
    }

    async fn delete_topic(&self, id: &str) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_topic(id).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_topic(id).await,
        }
    }

    async fn insert_chunks(&self, chunks: Vec<DocumentChunk>) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.insert_chunks(chunks).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.insert_chunks(chunks).await,
        }
    }

    async fn search_chunks(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> ApiResult<Vec<DocumentSearchResult>> {
        match self {
            Self::Libsql(store) => store.search_chunks(embedding, limit).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.search_chunks(embedding, limit).await,
        }
    }

    async fn delete_chunks_by_topic(&self, topic_id: &str) -> ApiResult<()> {
        match self {
            Self::Libsql(store) => store.delete_chunks_by_topic(topic_id).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.delete_chunks_by_topic(topic_id).await,
        }
    }

    async fn search_topics(&self, query: &str, limit: usize) -> ApiResult<Vec<SearchResult>> {
        match self {
            Self::Libsql(store) => store.search_topics(query, limit).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.search_topics(query, limit).await,
        }
    }

    async fn topics_by_id(&self, ids: &[String]) -> ApiResult<Vec<SearchResult>> {
        match self {
            Self::Libsql(store) => store.topics_by_id(ids).await,
            #[cfg(feature = "turso-experimental")]
            Self::Turso(store) => store.topics_by_id(ids).await,
        }
    }
}
