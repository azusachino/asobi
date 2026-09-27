//! HTTP implementation of the v3 traits (ADR 0005). One `reqwest::Client`
//! belongs to one process/store and is reused for the handshake and calls.

use asobi_core::api::v3::{
    API_VERSION, ApiError, ApiResult, BackendCapabilities, BackendHealth, BackendInfo, GraphStore,
    MaintenanceStore, OpenNodes, PurgeReport, PurgeRequest, SearchQuery, SearchStore, Stats,
    StorageLocation, TaskStore,
};
use asobi_core::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};
use asobi_core::protocol::{
    AddObservationsRequest, ClaimNextRequest, CreateEntitiesRequest, DeleteEntitiesRequest,
    DeleteObservationByIdRequest, DeleteObservationsRequest, DispatchRequest, EmptyRequest,
    ErrorBody, OpenNodesRequest, RelationsRequest, SearchNodesRequest, TruthDeleteRequest,
    TruthUpsertRequest, UpdateObservationByIdRequest, UpdateObservationRequest, error_body_to_api,
};
use reqwest::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RemoteError {
    pub error: ApiError,
    pub message: String,
    transport_failure: bool,
}

#[derive(Debug)]
pub enum ConnectError {
    Unreachable(String),
    Failure(RemoteError),
}

pub struct RemoteStore {
    client: Client,
    base: String,
    graph: String,
}

impl RemoteStore {
    pub async fn connect(remote: String, graph: String) -> Result<Self, ConnectError> {
        let base = remote.trim_end_matches('/').to_string();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(1)
            .build()
            .map_err(|e| {
                ConnectError::Failure(RemoteError {
                    error: ApiError::Backend(e.to_string()),
                    message: e.to_string(),
                    transport_failure: false,
                })
            })?;
        let store = Self {
            client,
            base,
            graph,
        };
        let hello: BackendInfo = match tokio::time::timeout(
            Duration::from_secs(2),
            store.call("server.hello", &EmptyRequest {}),
        )
        .await
        {
            Err(_) => {
                return Err(ConnectError::Unreachable(
                    "server.hello timed out after 2 seconds".to_string(),
                ));
            }
            Ok(Ok(hello)) => hello,
            Ok(Err(error)) if error.transport_failure => {
                return Err(ConnectError::Unreachable(error.message));
            }
            Ok(Err(error)) => return Err(ConnectError::Failure(error)),
        };
        if hello.api_version != API_VERSION {
            let message = format!(
                "remote API version mismatch: server is {}, this asobi requires {}",
                hello.api_version, API_VERSION
            );
            return Err(ConnectError::Failure(RemoteError {
                error: ApiError::Unsupported("remote API version mismatch"),
                message,
                transport_failure: false,
            }));
        }
        Ok(store)
    }

    async fn call<T: DeserializeOwned>(
        &self,
        operation: &str,
        body: &impl Serialize,
    ) -> Result<T, RemoteError> {
        let url = format!("{}/v3/graphs/{}/{operation}", self.base, self.graph);
        let response = self.client.post(url).json(body).send().await.map_err(|e| {
            let message = format!("remote server request failed: {e}");
            let transport_failure = e.is_connect() || e.is_timeout();
            RemoteError {
                error: if transport_failure {
                    ApiError::Unavailable(message.clone())
                } else {
                    ApiError::Invalid(message.clone())
                },
                message,
                transport_failure,
            }
        })?;
        let status = response.status();
        let bytes = response.bytes().await.map_err(|e| {
            let message = format!("failed to read remote response: {e}");
            RemoteError {
                error: ApiError::Unavailable(message.clone()),
                message,
                transport_failure: true,
            }
        })?;
        if status.is_success() {
            serde_json::from_slice(&bytes).map_err(|e| {
                let message = format!("invalid response from remote server: {e}");
                RemoteError {
                    error: ApiError::Backend(message.clone()),
                    message,
                    transport_failure: false,
                }
            })
        } else {
            let body: ErrorBody = serde_json::from_slice(&bytes).unwrap_or(ErrorBody {
                kind: "backend".to_string(),
                message: format!("remote server returned HTTP {status}"),
            });
            let error = error_body_to_api(&body).unwrap_or_else(|| match body.kind.as_str() {
                "notFound" | "unknownOperation" => ApiError::NotFound(body.message.clone()),
                "conflict" => ApiError::Conflict(body.message.clone()),
                "invalid" => ApiError::Invalid(body.message.clone()),
                "unsupported" => ApiError::Unsupported("remote operation unsupported"),
                "unavailable" => ApiError::Unavailable(body.message.clone()),
                "badRequest" => ApiError::Invalid(body.message.clone()),
                _ => ApiError::Backend(body.message.clone()),
            });
            Err(RemoteError {
                error,
                message: body.message,
                transport_failure: false,
            })
        }
    }

    async fn result<T: DeserializeOwned>(
        &self,
        operation: &str,
        body: &impl Serialize,
    ) -> ApiResult<T> {
        self.call(operation, body)
            .await
            .map_err(|error| error.error)
    }
}

macro_rules! remote_call {
    ($self:expr, $op:literal, $body:expr, $ret:ty) => {
        $self.result::<$ret>($op, &$body)
    };
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl GraphStore for RemoteStore {
    fn create_entities(
        &self,
        entities: Vec<EntityInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.createEntities",
                CreateEntitiesRequest { entities },
                ()
            )
            .await
        }
    }
    fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.addObservations",
                AddObservationsRequest {
                    observations,
                    limit
                },
                ()
            )
            .await
        }
    }
    fn create_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.createRelations",
                RelationsRequest { relations },
                ()
            )
            .await
        }
    }
    fn delete_entities(
        &self,
        names: Vec<String>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.deleteEntities",
                DeleteEntitiesRequest { names },
                ()
            )
            .await
        }
    }
    fn delete_observations(
        &self,
        deletions: Vec<ObservationDeletion>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.deleteObservations",
                DeleteObservationsRequest { deletions },
                ()
            )
            .await
        }
    }
    fn delete_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let entity_name = entity_name.to_string();
        async move {
            remote_call!(
                self,
                "graph.deleteObservationById",
                DeleteObservationByIdRequest { entity_name, id },
                ()
            )
            .await
        }
    }
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity_name, new_content) = (entity_name.to_string(), new_content.to_string());
        async move {
            remote_call!(
                self,
                "graph.updateObservationById",
                UpdateObservationByIdRequest {
                    entity_name,
                    id,
                    new_content
                },
                ()
            )
            .await
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
            remote_call!(
                self,
                "graph.updateObservation",
                UpdateObservationRequest {
                    entity_name,
                    old_content,
                    new_content
                },
                ()
            )
            .await
        }
    }
    fn delete_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move {
            remote_call!(
                self,
                "graph.deleteRelations",
                RelationsRequest { relations },
                ()
            )
            .await
        }
    }
    fn truth_upsert(
        &self,
        entity: &str,
        key: &str,
        value: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity, key, value) = (entity.to_string(), key.to_string(), value.to_string());
        async move {
            remote_call!(
                self,
                "graph.truthUpsert",
                TruthUpsertRequest { entity, key, value },
                ()
            )
            .await
        }
    }
    fn truth_delete(
        &self,
        entity: &str,
        key: &str,
    ) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        let (entity, key) = (entity.to_string(), key.to_string());
        async move {
            remote_call!(
                self,
                "graph.truthDelete",
                TruthDeleteRequest { entity, key },
                ()
            )
            .await
        }
    }
    fn read_graph(&self) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { remote_call!(self, "graph.readGraph", EmptyRequest {}, Graph).await }
    }
    fn read_graph_full(&self) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move { remote_call!(self, "graph.readGraphFull", EmptyRequest {}, Graph).await }
    }
    fn open_nodes(
        &self,
        req: OpenNodes,
    ) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move {
            remote_call!(
                self,
                "graph.openNodes",
                OpenNodesRequest {
                    names: req.names,
                    with_ids: req.with_ids,
                    expand: req.expand,
                    observation_limit: req.observation_limit
                },
                Graph
            )
            .await
        }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl SearchStore for RemoteStore {
    fn search_nodes(
        &self,
        query: SearchQuery,
    ) -> impl std::future::Future<Output = ApiResult<Graph>> + Send {
        async move {
            remote_call!(
                self,
                "search.nodes",
                SearchNodesRequest {
                    query: query.query,
                    limit: query.limit,
                    filters: query.filters
                },
                Graph
            )
            .await
        }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl MaintenanceStore for RemoteStore {
    fn stats(&self) -> impl std::future::Future<Output = ApiResult<Stats>> + Send {
        async move { remote_call!(self, "maintenance.stats", EmptyRequest {}, Stats).await }
    }
    fn stats_per_entity(
        &self,
    ) -> impl std::future::Future<Output = ApiResult<Vec<(String, usize)>>> + Send {
        async move {
            remote_call!(
                self,
                "maintenance.statsPerEntity",
                EmptyRequest {},
                Vec<(String, usize)>
            )
            .await
        }
    }
    fn purge(
        &self,
        request: PurgeRequest,
    ) -> impl std::future::Future<Output = ApiResult<PurgeReport>> + Send {
        async move { remote_call!(self, "maintenance.purge", request, PurgeReport).await }
    }
    fn reset(&self) -> impl std::future::Future<Output = ApiResult<()>> + Send {
        async move { remote_call!(self, "maintenance.reset", EmptyRequest {}, ()).await }
    }
    fn capabilities(
        &self,
    ) -> impl std::future::Future<Output = ApiResult<BackendCapabilities>> + Send {
        async move {
            remote_call!(
                self,
                "maintenance.capabilities",
                EmptyRequest {},
                BackendCapabilities
            )
            .await
        }
    }
    fn health(&self) -> impl std::future::Future<Output = ApiResult<BackendHealth>> + Send {
        async move { remote_call!(self, "maintenance.health", EmptyRequest {}, BackendHealth).await }
    }
    fn location(&self) -> impl std::future::Future<Output = ApiResult<StorageLocation>> + Send {
        async move {
            remote_call!(
                self,
                "maintenance.location",
                EmptyRequest {},
                StorageLocation
            )
            .await
        }
    }
}

#[allow(clippy::manual_async_fn)] // Keep the explicit Send future required by the v3 trait.
impl TaskStore for RemoteStore {
    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> impl std::future::Future<Output = ApiResult<Option<String>>> + Send {
        let (task, agent) = (task.map(str::to_string), agent.to_string());
        async move {
            remote_call!(
                self,
                "tasks.dispatch",
                DispatchRequest {
                    task,
                    agent,
                    observation_limit
                },
                Option<String>
            )
            .await
        }
    }
    fn claim_next(
        &self,
        agent: &str,
    ) -> impl std::future::Future<Output = ApiResult<Option<String>>> + Send {
        let agent = agent.to_string();
        async move {
            remote_call!(
                self,
                "tasks.claimNext",
                ClaimNextRequest { agent },
                Option<String>
            )
            .await
        }
    }
}
