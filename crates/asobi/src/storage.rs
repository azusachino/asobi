//! CLI-side provider selection. Remote transport lives here behind the
//! `remote` feature; the storage crate remains the local SQLite provider.

use asobi_core::api::v3::{
    ApiResult, BackendCapabilities, BackendHealth, GraphStore, MaintenanceStore, OpenNodes,
    PurgeReport, PurgeRequest, SearchQuery, SearchStore, Stats, StorageLocation, TaskStore,
};
use asobi_core::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};
use asobi_storage::SqliteStore;
use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "remote")]
mod remote;
#[cfg(feature = "remote")]
pub use remote::RemoteStore;

type CallFuture<'a, T> = Pin<Box<dyn Future<Output = ApiResult<T>> + Send + 'a>>;

#[cfg(feature = "remote")]
use asobi_core::api::v3::ApiError;

#[cfg(feature = "remote")]
enum Backend {
    Local(SqliteStore),
    /// No network request has happened yet. Negotiate before any graph call.
    Pending(RemoteStore),
    Remote(RemoteStore),
}

#[cfg(not(feature = "remote"))]
enum Backend {
    Local(SqliteStore),
}

/// One process selects exactly one backend. An unreachable configured remote
/// fails closed: no command silently opens the local graph.
pub struct Storage {
    backend: tokio::sync::Mutex<Backend>,
}

impl Storage {
    pub async fn is_remote(&self) -> bool {
        #[cfg(feature = "remote")]
        return matches!(
            &*self.backend.lock().await,
            Backend::Pending(_) | Backend::Remote(_)
        );
        #[cfg(not(feature = "remote"))]
        return false;
    }

    pub async fn open_local() -> crate::Result<Self> {
        Ok(Self {
            backend: tokio::sync::Mutex::new(Backend::Local(SqliteStore::open_default().await?)),
        })
    }

    pub async fn open_default() -> crate::Result<Self> {
        let paths = asobi_core::paths::AsobiPaths::resolve();
        let config = crate::config::resolve(&paths);
        if let Some(remote) = config.remote {
            #[cfg(feature = "remote")]
            {
                return Ok(Self {
                    backend: tokio::sync::Mutex::new(Backend::Pending(RemoteStore::new(
                        remote,
                        config.graph,
                    )?)),
                });
            }
            #[cfg(not(feature = "remote"))]
            {
                let _ = (remote, config.graph);
                anyhow::bail!("this asobi was built without remote support");
            }
        }
        Self::open_local().await
    }

    #[cfg(feature = "remote")]
    async fn route<T, R, L>(&self, _write: bool, remote_call: R, local_call: L) -> ApiResult<T>
    where
        T: Send,
        R: for<'a> FnOnce(&'a RemoteStore) -> CallFuture<'a, T> + Send,
        L: for<'a> FnOnce(&'a SqliteStore) -> CallFuture<'a, T> + Send,
    {
        let mut backend = self.backend.lock().await;
        match &*backend {
            Backend::Local(store) => return local_call(store).await,
            Backend::Remote(store) => return remote_call(store).await,
            Backend::Pending(_) => {}
        }

        let Backend::Pending(remote) = &*backend else {
            unreachable!()
        };
        let remote = remote.clone();
        // A single read-only negotiation precedes reads and writes alike.
        // The URL version is the accepted API major, not a package version.
        match tokio::time::timeout(std::time::Duration::from_secs(2), remote.negotiate()).await {
            Ok(Ok(())) => {
                *backend = Backend::Remote(remote.clone());
                remote_call(&remote).await
            }
            Ok(Err(ApiError::Unavailable(message))) => Err(ApiError::Unavailable(format!(
                "remote Asobi server unavailable: {message}"
            ))),
            Err(_) => Err(ApiError::Unavailable(
                "remote Asobi server unavailable: version negotiation timed out after 2 seconds"
                    .into(),
            )),
            Ok(Err(error)) => Err(error),
        }
    }

    #[cfg(not(feature = "remote"))]
    async fn route_local<T, L>(&self, local_call: L) -> ApiResult<T>
    where
        T: Send,
        L: for<'a> FnOnce(&'a SqliteStore) -> CallFuture<'a, T> + Send,
    {
        let backend = self.backend.lock().await;
        let Backend::Local(store) = &*backend;
        local_call(store).await
    }
}

#[cfg(feature = "remote")]
macro_rules! delegate {
    ($self:ident, $write:expr, $method:ident ( $($arg:expr),* $(,)? )) => {
        $self.route(
            $write,
            |store| Box::pin(store.$method($($arg.clone()),*)),
            |store| Box::pin(store.$method($($arg.clone()),*)),
        ).await
    };
}

#[cfg(not(feature = "remote"))]
macro_rules! delegate {
    ($self:ident, $write:expr, $method:ident ( $($arg:expr),* $(,)? )) => {{
        let _ = $write;
        $self.route_local(|store| Box::pin(store.$method($($arg),*))).await
    }};
}

#[allow(clippy::manual_async_fn)]
impl GraphStore for Storage {
    fn create_entities(
        &self,
        entities: Vec<EntityInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, create_entities(entities)) }
    }
    fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, add_observations(observations, limit)) }
    }
    fn create_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, create_relations(relations)) }
    }
    fn delete_entities(&self, names: Vec<String>) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, delete_entities(names)) }
    }
    fn delete_observations(
        &self,
        deletions: Vec<ObservationDeletion>,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, delete_observations(deletions)) }
    }
    fn delete_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        let entity_name = entity_name.to_string();
        async move {
            #[cfg(feature = "remote")]
            {
                let remote_name = entity_name.clone();
                let local_name = entity_name;
                self.route(
                    true,
                    move |store| {
                        Box::pin(
                            async move { store.delete_observation_by_id(&remote_name, id).await },
                        )
                    },
                    move |store| {
                        Box::pin(
                            async move { store.delete_observation_by_id(&local_name, id).await },
                        )
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move { store.delete_observation_by_id(&entity_name, id).await })
            })
            .await
        }
    }
    fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        let (entity_name, new_content) = (entity_name.to_string(), new_content.to_string());
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_name, remote_content) = (entity_name.clone(), new_content.clone());
                self.route(
                    true,
                    move |store| {
                        Box::pin(async move {
                            store
                                .update_observation_by_id(&remote_name, id, &remote_content)
                                .await
                        })
                    },
                    move |store| {
                        Box::pin(async move {
                            store
                                .update_observation_by_id(&entity_name, id, &new_content)
                                .await
                        })
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move {
                    store
                        .update_observation_by_id(&entity_name, id, &new_content)
                        .await
                })
            })
            .await
        }
    }
    fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        let (entity_name, old_content, new_content) = (
            entity_name.to_string(),
            old_content.to_string(),
            new_content.to_string(),
        );
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_name, remote_old, remote_new) = (
                    entity_name.clone(),
                    old_content.clone(),
                    new_content.clone(),
                );
                self.route(
                    true,
                    move |store| {
                        Box::pin(async move {
                            store
                                .update_observation(&remote_name, &remote_old, &remote_new)
                                .await
                        })
                    },
                    move |store| {
                        Box::pin(async move {
                            store
                                .update_observation(&entity_name, &old_content, &new_content)
                                .await
                        })
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move {
                    store
                        .update_observation(&entity_name, &old_content, &new_content)
                        .await
                })
            })
            .await
        }
    }
    fn delete_relations(
        &self,
        relations: Vec<RelationInput>,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, delete_relations(relations)) }
    }
    fn truth_upsert(
        &self,
        entity: &str,
        key: &str,
        value: &str,
    ) -> impl Future<Output = ApiResult<()>> + Send {
        let (entity, key, value) = (entity.to_string(), key.to_string(), value.to_string());
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_entity, remote_key, remote_value) =
                    (entity.clone(), key.clone(), value.clone());
                self.route(
                    true,
                    move |store| {
                        Box::pin(async move {
                            store
                                .truth_upsert(&remote_entity, &remote_key, &remote_value)
                                .await
                        })
                    },
                    move |store| {
                        Box::pin(async move { store.truth_upsert(&entity, &key, &value).await })
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move { store.truth_upsert(&entity, &key, &value).await })
            })
            .await
        }
    }
    fn truth_delete(&self, entity: &str, key: &str) -> impl Future<Output = ApiResult<()>> + Send {
        let (entity, key) = (entity.to_string(), key.to_string());
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_entity, remote_key) = (entity.clone(), key.clone());
                self.route(
                    true,
                    move |store| {
                        Box::pin(
                            async move { store.truth_delete(&remote_entity, &remote_key).await },
                        )
                    },
                    move |store| Box::pin(async move { store.truth_delete(&entity, &key).await }),
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move { store.truth_delete(&entity, &key).await })
            })
            .await
        }
    }
    fn read_graph(&self) -> impl Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, false, read_graph()) }
    }
    fn read_graph_full(&self) -> impl Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, false, read_graph_full()) }
    }
    fn open_nodes(&self, req: OpenNodes) -> impl Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, false, open_nodes(req)) }
    }
}

#[allow(clippy::manual_async_fn)]
impl SearchStore for Storage {
    fn search_nodes(&self, query: SearchQuery) -> impl Future<Output = ApiResult<Graph>> + Send {
        async move { delegate!(self, false, search_nodes(query)) }
    }
}

#[allow(clippy::manual_async_fn)]
impl MaintenanceStore for Storage {
    fn stats(&self) -> impl Future<Output = ApiResult<Stats>> + Send {
        async move { delegate!(self, false, stats()) }
    }
    fn stats_per_entity(&self) -> impl Future<Output = ApiResult<Vec<(String, usize)>>> + Send {
        async move { delegate!(self, false, stats_per_entity()) }
    }
    fn purge(&self, request: PurgeRequest) -> impl Future<Output = ApiResult<PurgeReport>> + Send {
        let write = request.apply;
        async move { delegate!(self, write, purge(request)) }
    }
    fn reset(&self) -> impl Future<Output = ApiResult<()>> + Send {
        async move { delegate!(self, true, reset()) }
    }
    fn capabilities(&self) -> impl Future<Output = ApiResult<BackendCapabilities>> + Send {
        async move { delegate!(self, false, capabilities()) }
    }
    fn health(&self) -> impl Future<Output = ApiResult<BackendHealth>> + Send {
        async move { delegate!(self, false, health()) }
    }
    fn location(&self) -> impl Future<Output = ApiResult<StorageLocation>> + Send {
        async move { delegate!(self, false, location()) }
    }
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for Storage {
    fn update(
        &self,
        task: &str,
        notes: Vec<String>,
        status: Option<&str>,
        observation_limit: usize,
    ) -> impl Future<Output = ApiResult<String>> + Send {
        let (task, status) = (task.to_string(), status.map(str::to_string));
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_task, remote_notes, remote_status) =
                    (task.clone(), notes.clone(), status.clone());
                self.route(
                    true,
                    move |store| {
                        Box::pin(async move {
                            store
                                .update(
                                    &remote_task,
                                    remote_notes,
                                    remote_status.as_deref(),
                                    observation_limit,
                                )
                                .await
                        })
                    },
                    move |store| {
                        Box::pin(async move {
                            store
                                .update(&task, notes, status.as_deref(), observation_limit)
                                .await
                        })
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move {
                    store
                        .update(&task, notes, status.as_deref(), observation_limit)
                        .await
                })
            })
            .await
        }
    }

    fn dispatch(
        &self,
        task: Option<&str>,
        agent: &str,
        observation_limit: usize,
    ) -> impl Future<Output = ApiResult<Option<String>>> + Send {
        let (task, agent) = (task.map(str::to_string), agent.to_string());
        async move {
            #[cfg(feature = "remote")]
            {
                let (remote_task, remote_agent) = (task.clone(), agent.clone());
                self.route(
                    true,
                    move |store| {
                        Box::pin(async move {
                            store
                                .dispatch(remote_task.as_deref(), &remote_agent, observation_limit)
                                .await
                        })
                    },
                    move |store| {
                        Box::pin(async move {
                            store
                                .dispatch(task.as_deref(), &agent, observation_limit)
                                .await
                        })
                    },
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| {
                Box::pin(async move {
                    store
                        .dispatch(task.as_deref(), &agent, observation_limit)
                        .await
                })
            })
            .await
        }
    }
    fn claim_next(&self, agent: &str) -> impl Future<Output = ApiResult<Option<String>>> + Send {
        let agent = agent.to_string();
        async move {
            #[cfg(feature = "remote")]
            {
                let remote_agent = agent.clone();
                self.route(
                    true,
                    move |store| Box::pin(async move { store.claim_next(&remote_agent).await }),
                    move |store| Box::pin(async move { store.claim_next(&agent).await }),
                )
                .await
            }
            #[cfg(not(feature = "remote"))]
            self.route_local(move |store| Box::pin(async move { store.claim_next(&agent).await }))
                .await
        }
    }
}
