//! One request body type per operation, named exactly as the ADR's table.
//! The body is a named JSON object whose fields are the trait method's
//! arguments; a field left out takes the same default the CLI uses, never a
//! zero that changes meaning. `()` results become a `null` body.

use crate::model::{EntityInput, ObservationDeletion, ObservationInput, RelationInput};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{RpcError, kinds};

/// The params for methods that take none: an empty object (or none at all).
#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
pub struct EmptyRequest {}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntitiesRequest {
    pub entities: Vec<EntityInput>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddObservationsRequest {
    pub observations: Vec<ObservationInput>,
    pub limit: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationsRequest {
    pub relations: Vec<RelationInput>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEntitiesRequest {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteObservationsRequest {
    pub deletions: Vec<ObservationDeletion>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteObservationByIdRequest {
    pub entity_name: String,
    pub id: i64,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateObservationByIdRequest {
    pub entity_name: String,
    pub id: i64,
    pub new_content: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateObservationRequest {
    pub entity_name: String,
    pub old_content: String,
    pub new_content: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthUpsertRequest {
    pub entity: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthDeleteRequest {
    pub entity: String,
    pub key: String,
}

#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DispatchRequest {
    pub task: Option<String>,
    pub agent: String,
    pub observation_limit: usize,
}

#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClaimNextRequest {
    pub agent: String,
}

/// The CLI's defaults, which an omitted request field takes (ADR 0005: an
/// omitted field never takes a zero that changes meaning).
pub const DEFAULT_SEARCH_LIMIT: usize = 10;
pub const DEFAULT_OBSERVATION_LIMIT: usize = 200;

fn default_search_limit() -> usize {
    DEFAULT_SEARCH_LIMIT
}

fn default_observation_limit() -> usize {
    DEFAULT_OBSERVATION_LIMIT
}

/// Request body for `graph.openNodes`.
#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpenNodesRequest {
    pub names: Vec<String>,
    pub with_ids: bool,
    pub expand: Vec<String>,
    #[serde(default = "default_observation_limit")]
    pub observation_limit: usize,
}

impl Default for OpenNodesRequest {
    fn default() -> Self {
        OpenNodesRequest {
            names: Vec::new(),
            with_ids: false,
            expand: Vec::new(),
            observation_limit: default_observation_limit(),
        }
    }
}

impl From<OpenNodesRequest> for crate::api::OpenNodes {
    fn from(request: OpenNodesRequest) -> Self {
        crate::api::OpenNodes {
            names: request.names,
            with_ids: request.with_ids,
            expand: request.expand,
            observation_limit: request.observation_limit,
        }
    }
}

/// Request body for `search.nodes`.
#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SearchNodesRequest {
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    pub filters: Vec<(String, String)>,
}

impl Default for SearchNodesRequest {
    fn default() -> Self {
        SearchNodesRequest {
            query: String::new(),
            limit: default_search_limit(),
            filters: Vec::new(),
        }
    }
}

impl From<SearchNodesRequest> for crate::api::SearchQuery {
    fn from(request: SearchNodesRequest) -> Self {
        crate::api::SearchQuery {
            query: request.query,
            limit: request.limit,
            filters: request.filters,
        }
    }
}

/// Parse the raw request body into a request type. A body that is not JSON or
/// does not match the method's declared shape is a 400 `badRequest`; absent
/// params deserialize from an empty object, so `{}`-shaped methods accept
/// both an empty body and `{}`.
pub(crate) fn parse<P: serde::de::DeserializeOwned>(body: Option<&[u8]>) -> Result<P, RpcError> {
    let value = match body {
        Some(bytes) => serde_json::from_slice::<Value>(bytes),
        None => Ok(Value::Object(serde_json::Map::new())),
    };
    let value = value.map_err(|e| RpcError::protocol(400, kinds::BAD_REQUEST, e.to_string()))?;
    serde_json::from_value(value)
        .map_err(|e| RpcError::protocol(400, kinds::BAD_REQUEST, e.to_string()))
}
