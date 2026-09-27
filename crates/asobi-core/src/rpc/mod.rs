//! The RPC contract (ADR 0005): method names, per-method params, the error
//! status/kind table, and an async dispatcher over the `v3` traits.
//!
//! Plain JSON over HTTP, no envelope: `POST /rpc/<graph>/<method>` with the
//! params object as the body answers 200 with the result JSON, or a non-2xx
//! status with `{"kind", "message"}`. This module is transport-free on
//! purpose — [`dispatch`] takes a method, the raw params bytes, and the store,
//! and returns either the result JSON or the [`RpcError`] (status + body).
//! WP4 wraps it in axum; WP5's `RemoteStore` speaks the same contract as a
//! client; both stay unit-testable against a temporary store.

mod error;
mod method;
mod params;

pub use error::{ErrorBody, RpcError, error_body_to_api};
pub use method::Method;
pub use params::{
    AddObservations, ClaimNext, CreateEntities, DeleteEntities, DeleteObservationById,
    DeleteObservations, Dispatch, Empty, Relations, TruthDelete, TruthUpsert, UpdateObservation,
    UpdateObservationById,
};

use crate::api::v3::{
    ApiError, BackendInfo, GraphStore, MaintenanceStore, OpenNodes, PurgeRequest, SearchQuery,
    SearchStore, Stats, TaskStore,
};
use serde::Serialize;
use serde_json::Value;

/// Answer one call: parse `body` as the method's params, call the trait
/// method, and serialize its result.
///
/// Transport-free: no socket, no HTTP types, no panic on bad input — the
/// [`RpcError`] (status + body) is the complete failure answer, so WP4's
/// handler is a thin status/body wrapper around whatever this returns.
pub async fn dispatch<S>(
    store: &S,
    method_name: &str,
    body: Option<&[u8]>,
) -> Result<Value, RpcError>
where
    S: GraphStore + SearchStore + MaintenanceStore + TaskStore,
{
    let Some(method) = Method::parse(method_name) else {
        return Err(RpcError::protocol(
            404,
            error::kinds::UNKNOWN_METHOD,
            format!("unknown method: {method_name}"),
        ));
    };
    handle(store, method, body).await
}

async fn handle<S>(store: &S, method: Method, body: Option<&[u8]>) -> Result<Value, RpcError>
where
    S: GraphStore + SearchStore + MaintenanceStore + TaskStore,
{
    use params as p;

    fn to_value<T: Serialize>(value: T) -> Result<Value, RpcError> {
        serde_json::to_value(value).map_err(|e| {
            RpcError::protocol(500, "backend", format!("result failed to serialize: {e}"))
        })
    }

    match method {
        Method::ServerHello => {
            p::parse::<p::Empty>(body)?;
            // One SQLite file per graph: the store's location is the state.
            let location = store.location().await?;
            let capabilities = store.capabilities().await?;
            to_value(BackendInfo {
                backend: capabilities.backend.clone(),
                api_version: crate::api::API_VERSION,
                schema_version: location.schema_version,
                state_id: location.database_path.clone(),
                capabilities,
            })
        }

        Method::GraphCreateEntities => {
            let params: p::CreateEntities = p::parse(body)?;
            store.create_entities(params.entities).await?;
            to_value(())
        }
        Method::GraphAddObservations => {
            let params: p::AddObservations = p::parse(body)?;
            store
                .add_observations(params.observations, params.limit)
                .await?;
            to_value(())
        }
        Method::GraphCreateRelations => {
            let params: p::Relations = p::parse(body)?;
            store.create_relations(params.relations).await?;
            to_value(())
        }
        Method::GraphDeleteRelations => {
            let params: p::Relations = p::parse(body)?;
            store.delete_relations(params.relations).await?;
            to_value(())
        }
        Method::GraphDeleteEntities => {
            let params: p::DeleteEntities = p::parse(body)?;
            store.delete_entities(params.names).await?;
            to_value(())
        }
        Method::GraphDeleteObservations => {
            let params: p::DeleteObservations = p::parse(body)?;
            store.delete_observations(params.deletions).await?;
            to_value(())
        }
        Method::GraphDeleteObservationById => {
            let params: p::DeleteObservationById = p::parse(body)?;
            store
                .delete_observation_by_id(&params.entity_name, params.id)
                .await?;
            to_value(())
        }
        Method::GraphUpdateObservationById => {
            let params: p::UpdateObservationById = p::parse(body)?;
            store
                .update_observation_by_id(&params.entity_name, params.id, &params.new_content)
                .await?;
            to_value(())
        }
        Method::GraphUpdateObservation => {
            let params: p::UpdateObservation = p::parse(body)?;
            store
                .update_observation(
                    &params.entity_name,
                    &params.old_content,
                    &params.new_content,
                )
                .await?;
            to_value(())
        }
        Method::GraphTruthUpsert => {
            let params: p::TruthUpsert = p::parse(body)?;
            store
                .truth_upsert(&params.entity, &params.key, &params.value)
                .await?;
            to_value(())
        }
        Method::GraphTruthDelete => {
            let params: p::TruthDelete = p::parse(body)?;
            store.truth_delete(&params.entity, &params.key).await?;
            to_value(())
        }
        Method::GraphReadGraph => {
            p::parse::<p::Empty>(body)?;
            to_value(store.read_graph().await?)
        }
        Method::GraphReadGraphFull => {
            p::parse::<p::Empty>(body)?;
            to_value(store.read_graph_full().await?)
        }
        Method::GraphOpenNodes => {
            let params: crate::api::OpenNodes = p::parse(body)?;
            to_value(store.open_nodes(params).await?)
        }
        Method::SearchNodes => {
            let params: SearchQuery = p::parse(body)?;
            to_value(store.search_nodes(params).await?)
        }

        Method::MaintenanceStats => {
            p::parse::<p::Empty>(body)?;
            to_value(store.stats().await?)
        }
        Method::MaintenanceStatsPerEntity => {
            p::parse::<p::Empty>(body)?;
            to_value(store.stats_per_entity().await?)
        }
        Method::MaintenancePurge => {
            let params: PurgeRequest = p::parse(body)?;
            to_value(store.purge(params).await?)
        }
        Method::MaintenanceReset => {
            p::parse::<p::Empty>(body)?;
            Err(ApiError::Unsupported(
                "reset is local-only over RPC; run it on the server host against the file directly",
            )
            .to_rpc_error())
        }
        Method::MaintenanceCapabilities => {
            p::parse::<p::Empty>(body)?;
            to_value(store.capabilities().await?)
        }
        Method::MaintenanceHealth => {
            p::parse::<p::Empty>(body)?;
            to_value(store.health().await?)
        }
        Method::MaintenanceLocation => {
            p::parse::<p::Empty>(body)?;
            to_value(store.location().await?)
        }

        Method::TasksDispatch => {
            let params: p::Dispatch = p::parse(body)?;
            to_value(
                store
                    .dispatch(
                        params.task.as_deref(),
                        &params.agent,
                        params.observation_limit,
                    )
                    .await?,
            )
        }
        Method::TasksClaimNext => {
            let params: p::ClaimNext = p::parse(body)?;
            to_value(store.claim_next(&params.agent).await?)
        }
    }
}

/// Params and result schemas for every method, published by `asobi schema`.
/// Walks [`Method::all`] so a new method cannot be added half-way: forgetting
/// its row here fails the round-trip test that consumes this list.
pub fn method_schemas() -> Vec<(&'static str, Value, Value)> {
    fn schema<T: schemars::JsonSchema>() -> Value {
        serde_json::to_value(schemars::schema_for!(T)).expect("schemas must serialize to JSON")
    }
    fn row<P: schemars::JsonSchema, R: schemars::JsonSchema>(
        method: Method,
    ) -> (&'static str, Value, Value) {
        (method.name(), schema::<P>(), schema::<R>())
    }
    Method::all()
        .map(|method| match method {
            Method::ServerHello => row::<params::Empty, BackendInfo>(method),
            Method::GraphCreateEntities => row::<params::CreateEntities, ()>(method),
            Method::GraphAddObservations => row::<params::AddObservations, ()>(method),
            Method::GraphCreateRelations | Method::GraphDeleteRelations => {
                row::<params::Relations, ()>(method)
            }
            Method::GraphDeleteEntities => row::<params::DeleteEntities, ()>(method),
            Method::GraphDeleteObservations => row::<params::DeleteObservations, ()>(method),
            Method::GraphDeleteObservationById => row::<params::DeleteObservationById, ()>(method),
            Method::GraphUpdateObservationById => row::<params::UpdateObservationById, ()>(method),
            Method::GraphUpdateObservation => row::<params::UpdateObservation, ()>(method),
            Method::GraphTruthUpsert => row::<params::TruthUpsert, ()>(method),
            Method::GraphTruthDelete => row::<params::TruthDelete, ()>(method),
            Method::GraphReadGraph | Method::GraphReadGraphFull => {
                row::<params::Empty, crate::model::Graph>(method)
            }
            Method::GraphOpenNodes => row::<OpenNodes, crate::model::Graph>(method),
            Method::SearchNodes => row::<SearchQuery, crate::model::Graph>(method),
            Method::MaintenanceStats => row::<params::Empty, Stats>(method),
            Method::MaintenanceStatsPerEntity => row::<params::Empty, Vec<(String, usize)>>(method),
            Method::MaintenancePurge => row::<PurgeRequest, crate::api::PurgeReport>(method),
            Method::MaintenanceReset => row::<params::Empty, ()>(method),
            Method::MaintenanceCapabilities => {
                row::<params::Empty, crate::api::BackendCapabilities>(method)
            }
            Method::MaintenanceHealth => row::<params::Empty, crate::api::BackendHealth>(method),
            Method::MaintenanceLocation => {
                row::<params::Empty, crate::api::StorageLocation>(method)
            }
            Method::TasksDispatch => row::<params::Dispatch, Option<String>>(method),
            Method::TasksClaimNext => row::<params::ClaimNext, Option<String>>(method),
        })
        .collect()
}
