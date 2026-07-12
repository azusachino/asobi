//! Domain types for the knowledge graph, shared across `db`, `main`, and
//! `backup`. These are the canonical in-memory shapes for graph I/O; the JSON
//! field names (camelCase) are the stable serialization contract for
//! `graph` / `search` / `show` / `export`; CLI responses wrap these data shapes
//! in the versioned `{schemaVersion, ok, data|error}` envelope.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityInput {
    pub name: String,
    pub entity_type: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelationInput {
    pub from: String,
    pub to: String,
    pub relation_type: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ObservationInput {
    pub entity_name: String,
    pub contents: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ObservationDeletion {
    pub entity_name: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetailedObservation {
    pub id: i64,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityOutput {
    pub name: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub observations: Vec<String>,
    pub truths: std::collections::BTreeMap<String, String>,
    pub observation_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observations_detailed: Option<Vec<DetailedObservation>>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub entities: Vec<EntityOutput>,
    pub relations: Vec<RelationInput>,
}
