use async_trait::async_trait;

use crate::api::GraphStore;
use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

pub struct TursoBackend {
    db: turso::Database,
    conn: turso::Connection,
}

impl TursoBackend {
    pub async fn open() -> crate::Result<Self> {
        let (db, conn) = crate::db::init_db().await?;
        Ok(Self { db, conn })
    }

    pub fn from_parts(db: turso::Database, conn: turso::Connection) -> Self {
        Self { db, conn }
    }

    pub fn into_parts(self) -> (turso::Database, turso::Connection) {
        (self.db, self.conn)
    }

    pub fn database(&self) -> &turso::Database {
        &self.db
    }

    pub fn connection(&self) -> &turso::Connection {
        &self.conn
    }
}

#[async_trait(?Send)]
impl GraphStore for TursoBackend {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> crate::Result<()> {
        crate::db::create_entities(&self.conn, entities).await
    }

    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> crate::Result<()> {
        crate::db::add_observations(&self.conn, observations, limit).await
    }

    async fn create_relations(&self, relations: Vec<RelationInput>) -> crate::Result<()> {
        crate::db::create_relations(&self.conn, relations).await
    }

    async fn delete_entities(&self, names: Vec<String>) -> crate::Result<()> {
        crate::db::delete_entities(&self.conn, names).await
    }

    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> crate::Result<()> {
        crate::db::delete_observations(&self.conn, deletions).await
    }

    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> crate::Result<()> {
        crate::db::delete_observation_by_id(&self.conn, entity_name, id).await
    }

    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> crate::Result<()> {
        crate::db::update_observation_by_id(&self.conn, entity_name, id, new_content).await
    }

    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> crate::Result<()> {
        crate::db::update_observation(&self.conn, entity_name, old_content, new_content).await
    }

    async fn delete_relations(&self, relations: Vec<RelationInput>) -> crate::Result<()> {
        crate::db::delete_relations(&self.conn, relations).await
    }

    async fn read_graph(&self) -> crate::Result<Graph> {
        crate::db::read_graph(&self.conn).await
    }

    async fn read_graph_eager(&self) -> crate::Result<Graph> {
        crate::db::read_graph_eager(&self.conn).await
    }

    async fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> crate::Result<Graph> {
        crate::db::read_graph_scoped(&self.conn, scope, rationale).await
    }

    async fn search_nodes(
        &self,
        query: &str,
        limit: usize,
        filters: &[(String, String)],
    ) -> crate::Result<Graph> {
        crate::db::search_nodes_with_limit(&self.conn, query, limit, filters).await
    }

    async fn open_nodes_detailed(
        &self,
        names: Vec<String>,
        with_ids: bool,
        expand: &[String],
    ) -> crate::Result<Graph> {
        crate::db::open_nodes_detailed(&self.conn, names, with_ids, expand).await
    }

    async fn open_nodes(&self, names: Vec<String>) -> crate::Result<Graph> {
        crate::db::open_nodes(&self.conn, names).await
    }

    async fn stats(&self) -> crate::Result<(usize, usize, usize)> {
        crate::db::stats(&self.conn).await
    }

    async fn stats_per_entity(&self) -> crate::Result<Vec<(String, usize)>> {
        crate::db::stats_per_entity(&self.conn).await
    }

    async fn reset(&self) -> crate::Result<()> {
        crate::db::reset(&self.conn).await
    }

    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> crate::Result<()> {
        crate::db::truth_upsert(&self.conn, entity, key, value).await
    }

    async fn truth_delete(&self, entity: &str, key: &str) -> crate::Result<()> {
        crate::db::truth_delete(&self.conn, entity, key).await
    }
}
