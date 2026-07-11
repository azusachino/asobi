//! Backend-neutral graph storage boundary.
//!
//! Driver-specific SQL and value/row handling stays in the db module. The CLI
//! depends on this domain-shaped interface so another backend can be added
//! without exposing a database driver's connection throughout the command
//! layer.

use async_trait::async_trait;

use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

#[async_trait(?Send)]
pub trait GraphStore {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> crate::Result<()>;
    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> crate::Result<()>;
    async fn create_relations(&self, relations: Vec<RelationInput>) -> crate::Result<()>;
    async fn delete_entities(&self, names: Vec<String>) -> crate::Result<()>;
    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> crate::Result<()>;
    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> crate::Result<()>;
    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> crate::Result<()>;
    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> crate::Result<()>;
    async fn delete_relations(&self, relations: Vec<RelationInput>) -> crate::Result<()>;
    async fn read_graph(&self) -> crate::Result<Graph>;
    async fn read_graph_eager(&self) -> crate::Result<Graph>;
    async fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> crate::Result<Graph>;
    async fn search_nodes(
        &self,
        query: &str,
        limit: usize,
        filters: &[(String, String)],
    ) -> crate::Result<Graph>;
    async fn open_nodes_detailed(
        &self,
        names: Vec<String>,
        with_ids: bool,
        expand: &[String],
    ) -> crate::Result<Graph>;
    async fn open_nodes(&self, names: Vec<String>) -> crate::Result<Graph>;
    async fn stats(&self) -> crate::Result<(usize, usize, usize)>;
    async fn stats_per_entity(&self) -> crate::Result<Vec<(String, usize)>>;
    async fn reset(&self) -> crate::Result<()>;
    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> crate::Result<()>;
    async fn truth_delete(&self, entity: &str, key: &str) -> crate::Result<()>;
}
