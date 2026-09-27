//! CLI-side provider selection. Remote transport belongs to this crate and is
//! feature-gated; the storage crate remains the local SQLite provider only.

use asobi_core::api::v3::{
    ApiResult, BackendCapabilities, BackendHealth, GraphStore, MaintenanceStore, OpenNodes,
    PurgeReport, PurgeRequest, SearchQuery, SearchStore, Stats, StorageLocation, TaskStore,
};
use asobi_core::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};
use asobi_storage::SqliteStore;

#[cfg(feature = "remote")]
mod remote;
#[cfg(feature = "remote")]
pub use remote::RemoteStore;

/// A process uses exactly one backend. Selection happens before dispatch and
/// never changes after opening, so a command cannot split writes across modes.
pub enum Storage {
    Local(SqliteStore),
    #[cfg(feature = "remote")]
    Remote(RemoteStore),
}

impl Storage {
    pub async fn open_default() -> crate::Result<Self> {
        let paths = asobi_core::paths::AsobiPaths::resolve();
        let config = crate::config::resolve(&paths);
        if let Some(remote) = config.remote {
            #[cfg(feature = "remote")]
            {
                match RemoteStore::connect(remote, config.graph).await {
                    Ok(store) => return Ok(Self::Remote(store)),
                    Err(remote::ConnectError::Unreachable(error)) => {
                        eprintln!(
                            "warning: remote Asobi server unavailable; this command is using the local graph, and its writes will not reach the server ({error})"
                        );
                    }
                    Err(remote::ConnectError::Failure(error)) => return Err(error.into()),
                }
            }
            #[cfg(not(feature = "remote"))]
            {
                let _ = (remote, config.graph);
                anyhow::bail!("this asobi was built without remote support");
            }
        }
        Ok(Self::Local(SqliteStore::open_default().await?))
    }
}

macro_rules! delegate {
    ($self:ident, $method:ident ( $($arg:expr),* $(,)? )) => {
        match $self {
            Storage::Local(store) => store.$method($($arg),*).await,
            #[cfg(feature = "remote")]
            Storage::Remote(store) => store.$method($($arg),*).await,
        }
    };
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl GraphStore for Storage {
    fn create_entities(
        &self,
        entities: Vec<EntityInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, create_entities(entities)) }
    }
    fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, add_observations(observations, limit)) }
    }
    fn create_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, create_relations(relations)) }
    }
    fn delete_entities(
        &self,
        names: Vec<String>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, delete_entities(names)) }
    }
    fn delete_observations(
        &self,
        deletions: Vec<ObservationDeletion>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, delete_observations(deletions)) }
    }
    fn delete_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let entity_name = entity_name.to_string();
        async move { delegate!(self, delete_observation_by_id(&entity_name, id)) }
    }
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity_name, new_content) = (entity_name.to_string(), new_content.to_string());
        async move {
            delegate!(
                self,
                update_observation_by_id(&entity_name, id, &new_content)
            )
        }
    }
    fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity_name, old_content, new_content) = (
            entity_name.to_string(),
            old_content.to_string(),
            new_content.to_string(),
        );
        async move {
            delegate!(
                self,
                update_observation(&entity_name, &old_content, &new_content)
            )
        }
    }
    fn delete_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, delete_relations(relations)) }
    }
    fn truth_upsert(
        &self,
        entity: &str,
        key: &str,
        value: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity, key, value) = (entity.to_string(), key.to_string(), value.to_string());
        async move { delegate!(self, truth_upsert(&entity, &key, &value)) }
    }
    fn truth_delete(
        &self,
        entity: &str,
        key: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity, key) = (entity.to_string(), key.to_string());
        async move { delegate!(self, truth_delete(&entity, &key)) }
    }
    fn read_graph(&self) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, read_graph()) }
    }
    fn read_graph_full(&self) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, read_graph_full()) }
    }
    fn open_nodes(
        &self,
        req: OpenNodes,
    ) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, open_nodes(req)) }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl SearchStore for Storage {
    fn search_nodes(
        &self,
        query: SearchQuery,
    ) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, search_nodes(query)) }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl MaintenanceStore for Storage {
    fn stats(&self) -> impl std::future::Future<Output = ApiResult<Stats>> + Send {
        async move { delegate!(self, stats()) }
    }
    fn stats_per_entity(
        &self,
    ) -> impl std::future::Future<Output = ApiResult<Vec<(String, usize)>>> + Send {
        async move { delegate!(self, stats_per_entity()) }
    }
    fn purge(
        &self,
        request: PurgeRequest,
    ) -> impl std::future::Future<Output = ApiResult<PurgeReport>> + Send {
        async move { delegate!(self, purge(request)) }
    }
    fn reset(&self) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, reset()) }
    }
    fn capabilities(
        &self,
    ) -> impl std::future::Future<Output = ApiResult<BackendCapabilities>> + Send {
        async move { delegate!(self, capabilities()) }
    }
    fn health(&self) -> impl std::future::Future<Output = ApiResult<BackendHealth>> + Send {
        async move { delegate!(self, health()) }
    }
    fn location(&self) -> impl std::future::Future<Output = ApiResult<StorageLocation>> + Send {
        async move { delegate!(self, location()) }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl TaskStore for Storage {
    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> impl std::future::Future<Output = ApiResult<Option<String>>> + Send {
        let (task, agent) = (task.map(str::to_string), agent.to_string());
        async move { delegate!(self, dispatch(task.as_deref(), &agent, observation_limit)) }
    }
    fn claim_next(
        &self,
        agent: &str,
    ) -> impl std::future::Future<Output = ApiResult<Option<String>>> + Send {
        let agent = agent.to_string();
        async move { delegate!(self, claim_next(&agent)) }
    }
}
