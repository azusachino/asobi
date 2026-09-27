//! One params type per RPC method, named exactly as the ADR's table. Params
//! travel as the request body: a named JSON object whose fields are the trait
//! method's arguments. `()` results become a `null` body.

use crate::model::{EntityInput, ObservationDeletion, ObservationInput, RelationInput};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{RpcError, kinds};

/// The params for methods that take none: an empty object (or none at all).
#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
pub struct Empty {}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntities {
    pub entities: Vec<EntityInput>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddObservations {
    pub observations: Vec<ObservationInput>,
    pub limit: usize,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relations {
    pub relations: Vec<RelationInput>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEntities {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteObservations {
    pub deletions: Vec<ObservationDeletion>,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteObservationById {
    pub entity_name: String,
    pub id: i64,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateObservationById {
    pub entity_name: String,
    pub id: i64,
    pub new_content: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateObservation {
    pub entity_name: String,
    pub old_content: String,
    pub new_content: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthUpsert {
    pub entity: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TruthDelete {
    pub entity: String,
    pub key: String,
}

#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dispatch {
    pub task: Option<String>,
    pub agent: String,
    pub observation_limit: usize,
}

#[derive(Debug, Clone, Default, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimNext {
    pub agent: String,
}

/// Parse the raw request body into a params type. A body that is not JSON or
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
