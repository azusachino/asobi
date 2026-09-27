//! The HTTP protocol contract (ADR 0005): operation names, per-operation
//! request bodies, the error status/kind table, and an async dispatcher over
//! the `v3` traits.
//!
//! Plain JSON over HTTP, no envelope: `POST /v3/graphs/<graph>/<operation>`
//! with the request body as the payload answers 200 with the result JSON, or
//! a non-2xx status with `{"kind", "message"}`. This module is transport-free
//! on purpose — [`dispatch`] takes the graph name, the operation, the raw
//! body bytes, and the store, and returns either the result JSON or the
//! [`RpcError`] (status + body). WP4 wraps it in axum; WP5's `RemoteStore`
//! speaks the same contract as a client; both stay unit-testable against a
//! temporary store.

mod error;
mod operation;
mod requests;

pub use error::{ErrorBody, RpcError, error_body_to_api};
pub use operation::Operation;
pub use requests::{
    AddObservationsRequest, ClaimNextRequest, CreateEntitiesRequest, DeleteEntitiesRequest,
    DeleteObservationByIdRequest, DeleteObservationsRequest, DispatchRequest, EmptyRequest,
    RelationsRequest, TruthDeleteRequest, TruthUpsertRequest, UpdateObservationByIdRequest,
    UpdateObservationRequest,
};

use crate::api::v3::{
    ApiError, BackendInfo, GraphStore, MaintenanceStore, PurgeRequest, SearchQuery, SearchStore,
    Stats, TaskStore,
};
use serde::Serialize;
use serde_json::Value;

/// Answer one call: parse `body` as the operation's request, call the trait
/// method, and serialize its result.
///
/// Transport-free: no socket, no HTTP types, no panic on bad input — the
/// [`RpcError`] (status + body) is the complete failure answer, so WP4's
/// handler is a thin status/body wrapper around whatever this returns.
pub async fn dispatch<S>(
    store: &S,
    graph: &str,
    operation_name: &str,
    body: Option<&[u8]>,
) -> Result<Value, RpcError>
where
    S: GraphStore + SearchStore + MaintenanceStore + TaskStore,
{
    let Some(operation) = Operation::parse(operation_name) else {
        return Err(RpcError::protocol(
            404,
            error::kinds::UNKNOWN_OPERATION,
            format!("unknown operation: {operation_name}"),
        ));
    };
    handle(store, graph, operation, body).await
}

async fn handle<S>(
    store: &S,
    graph: &str,
    operation: Operation,
    body: Option<&[u8]>,
) -> Result<Value, RpcError>
where
    S: GraphStore + SearchStore + MaintenanceStore + TaskStore,
{
    fn to_value<T: Serialize>(value: T) -> Result<Value, RpcError> {
        serde_json::to_value(value).map_err(|e| {
            RpcError::protocol(500, "backend", format!("result failed to serialize: {e}"))
        })
    }

    match operation {
        Operation::ServerHello => {
            requests::parse::<requests::EmptyRequest>(body)?;
            // One SQLite file per graph: the store's location is the state.
            let location = store.location().await?;
            let capabilities = store.capabilities().await?;
            to_value(BackendInfo {
                backend: capabilities.backend.clone(),
                api_version: crate::api::API_VERSION,
                schema_version: location.schema_version,
                // The graph name identifies the state; server file paths never
                // leave the server (ADR 0005).
                state_id: graph.to_string(),
                capabilities,
            })
        }

        Operation::GraphCreateEntities => {
            let request: requests::CreateEntitiesRequest = requests::parse(body)?;
            store.create_entities(request.entities).await?;
            to_value(())
        }
        Operation::GraphAddObservations => {
            let request: requests::AddObservationsRequest = requests::parse(body)?;
            store
                .add_observations(request.observations, request.limit)
                .await?;
            to_value(())
        }
        Operation::GraphCreateRelations => {
            let request: requests::RelationsRequest = requests::parse(body)?;
            store.create_relations(request.relations).await?;
            to_value(())
        }
        Operation::GraphDeleteRelations => {
            let request: requests::RelationsRequest = requests::parse(body)?;
            store.delete_relations(request.relations).await?;
            to_value(())
        }
        Operation::GraphDeleteEntities => {
            let request: requests::DeleteEntitiesRequest = requests::parse(body)?;
            store.delete_entities(request.names).await?;
            to_value(())
        }
        Operation::GraphDeleteObservations => {
            let request: requests::DeleteObservationsRequest = requests::parse(body)?;
            store.delete_observations(request.deletions).await?;
            to_value(())
        }
        Operation::GraphDeleteObservationById => {
            let request: requests::DeleteObservationByIdRequest = requests::parse(body)?;
            store
                .delete_observation_by_id(&request.entity_name, request.id)
                .await?;
            to_value(())
        }
        Operation::GraphUpdateObservationById => {
            let request: requests::UpdateObservationByIdRequest = requests::parse(body)?;
            store
                .update_observation_by_id(&request.entity_name, request.id, &request.new_content)
                .await?;
            to_value(())
        }
        Operation::GraphUpdateObservation => {
            let request: requests::UpdateObservationRequest = requests::parse(body)?;
            store
                .update_observation(
                    &request.entity_name,
                    &request.old_content,
                    &request.new_content,
                )
                .await?;
            to_value(())
        }
        Operation::GraphTruthUpsert => {
            let request: requests::TruthUpsertRequest = requests::parse(body)?;
            store
                .truth_upsert(&request.entity, &request.key, &request.value)
                .await?;
            to_value(())
        }
        Operation::GraphTruthDelete => {
            let request: requests::TruthDeleteRequest = requests::parse(body)?;
            store.truth_delete(&request.entity, &request.key).await?;
            to_value(())
        }
        Operation::GraphReadGraph => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.read_graph().await?)
        }
        Operation::GraphReadGraphFull => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.read_graph_full().await?)
        }
        Operation::GraphOpenNodes => {
            let request: crate::api::OpenNodes = requests::parse(body)?;
            to_value(store.open_nodes(request).await?)
        }
        Operation::SearchNodes => {
            let request: SearchQuery = requests::parse(body)?;
            to_value(store.search_nodes(request).await?)
        }

        Operation::MaintenanceStats => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.stats().await?)
        }
        Operation::MaintenanceStatsPerEntity => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.stats_per_entity().await?)
        }
        Operation::MaintenancePurge => {
            let request: PurgeRequest = requests::parse(body)?;
            to_value(store.purge(request).await?)
        }
        Operation::MaintenanceReset => {
            requests::parse::<requests::EmptyRequest>(body)?;
            Err(ApiError::Unsupported(
                "reset is not available over the network; run `asobi reset` on the server host",
            )
            .to_rpc_error())
        }
        Operation::MaintenanceCapabilities => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.capabilities().await?)
        }
        Operation::MaintenanceHealth => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.health().await?)
        }
        Operation::MaintenanceLocation => {
            requests::parse::<requests::EmptyRequest>(body)?;
            to_value(store.location().await?)
        }

        Operation::TasksDispatch => {
            let request: requests::DispatchRequest = requests::parse(body)?;
            to_value(
                store
                    .dispatch(
                        request.task.as_deref(),
                        &request.agent,
                        request.observation_limit,
                    )
                    .await?,
            )
        }
        Operation::TasksClaimNext => {
            let request: requests::ClaimNextRequest = requests::parse(body)?;
            to_value(store.claim_next(&request.agent).await?)
        }
    }
}

/// Params and result schemas for every method, published by `asobi schema`.
/// Walks [`Operation::all`] so a new method cannot be added half-way: forgetting
/// its row here fails the round-trip test that consumes this list.
pub fn method_schemas() -> Vec<(&'static str, Value, Value)> {
    fn schema<T: schemars::JsonSchema>() -> Value {
        serde_json::to_value(schemars::schema_for!(T)).expect("schemas must serialize to JSON")
    }
    fn row<P: schemars::JsonSchema, R: schemars::JsonSchema>(
        operation: Operation,
    ) -> (&'static str, Value, Value) {
        (operation.name(), schema::<P>(), schema::<R>())
    }
    Operation::all()
        .map(|operation| match operation {
            Operation::ServerHello => row::<requests::EmptyRequest, BackendInfo>(operation),
            Operation::GraphCreateEntities => row::<requests::CreateEntitiesRequest, ()>(operation),
            Operation::GraphAddObservations => {
                row::<requests::AddObservationsRequest, ()>(operation)
            }
            Operation::GraphCreateRelations | Operation::GraphDeleteRelations => {
                row::<requests::RelationsRequest, ()>(operation)
            }
            Operation::GraphDeleteEntities => row::<requests::DeleteEntitiesRequest, ()>(operation),
            Operation::GraphDeleteObservations => {
                row::<requests::DeleteObservationsRequest, ()>(operation)
            }
            Operation::GraphDeleteObservationById => {
                row::<requests::DeleteObservationByIdRequest, ()>(operation)
            }
            Operation::GraphUpdateObservationById => {
                row::<requests::UpdateObservationByIdRequest, ()>(operation)
            }
            Operation::GraphUpdateObservation => {
                row::<requests::UpdateObservationRequest, ()>(operation)
            }
            Operation::GraphTruthUpsert => row::<requests::TruthUpsertRequest, ()>(operation),
            Operation::GraphTruthDelete => row::<requests::TruthDeleteRequest, ()>(operation),
            Operation::GraphReadGraph | Operation::GraphReadGraphFull => {
                row::<requests::EmptyRequest, crate::model::Graph>(operation)
            }
            Operation::GraphOpenNodes => {
                row::<requests::OpenNodesRequest, crate::model::Graph>(operation)
            }
            Operation::SearchNodes => {
                row::<requests::SearchNodesRequest, crate::model::Graph>(operation)
            }
            Operation::MaintenanceStats => row::<requests::EmptyRequest, Stats>(operation),
            Operation::MaintenanceStatsPerEntity => {
                row::<requests::EmptyRequest, Vec<(String, usize)>>(operation)
            }
            Operation::MaintenancePurge => row::<PurgeRequest, crate::api::PurgeReport>(operation),
            Operation::MaintenanceReset => row::<requests::EmptyRequest, ()>(operation),
            Operation::MaintenanceCapabilities => {
                row::<requests::EmptyRequest, crate::api::BackendCapabilities>(operation)
            }
            Operation::MaintenanceHealth => {
                row::<requests::EmptyRequest, crate::api::BackendHealth>(operation)
            }
            Operation::MaintenanceLocation => {
                row::<requests::EmptyRequest, crate::api::StorageLocation>(operation)
            }
            Operation::TasksDispatch => row::<requests::DispatchRequest, Option<String>>(operation),
            Operation::TasksClaimNext => {
                row::<requests::ClaimNextRequest, Option<String>>(operation)
            }
        })
        .collect()
}
