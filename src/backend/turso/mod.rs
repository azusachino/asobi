//! Turso backend: the SQLite-dialect implementation of the `api::v1` contract.
//!
//! All SQL, schema, indexes, and driver handling live here and below (see
//! `crate::backend::turso::db` / `crate::backend::turso::tx`); the API layer above never sees a connection.

pub mod constant;
pub mod db;
pub mod tx;
#[cfg(feature = "documents")]
pub mod vector;

use crate::api::v1::{
    ApiError, ApiResult, BackendCapabilities, BackendHealth, DocumentChunk, DocumentSearchResult,
    DocumentStore, GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchResult, SearchStore,
    Snapshot, SnapshotStore, Stats, TopicSnapshot,
};
use crate::model::{EntityInput, Graph, ObservationDeletion, ObservationInput, RelationInput};

/// Map an internal (anyhow) error onto the stable API error surface.
fn be(e: anyhow::Error) -> ApiError {
    ApiError::Backend(e.to_string())
}

pub struct TursoBackend {
    db: turso::Database,
    conn: turso::Connection,
}

impl TursoBackend {
    pub async fn open() -> crate::Result<Self> {
        let (db, conn) = crate::backend::turso::db::init_db().await?;
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

impl GraphStore for TursoBackend {
    async fn create_entities(&self, entities: Vec<EntityInput>) -> ApiResult<()> {
        crate::backend::turso::db::create_entities(&self.conn, entities)
            .await
            .map_err(be)
    }

    async fn add_observations(
        &self,
        observations: Vec<ObservationInput>,
        limit: usize,
    ) -> ApiResult<()> {
        crate::backend::turso::db::add_observations(&self.conn, observations, limit)
            .await
            .map_err(be)
    }

    async fn create_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        crate::backend::turso::db::create_relations(&self.conn, relations)
            .await
            .map_err(be)
    }

    async fn delete_entities(&self, names: Vec<String>) -> ApiResult<()> {
        crate::backend::turso::db::delete_entities(&self.conn, names)
            .await
            .map_err(be)
    }

    async fn delete_observations(&self, deletions: Vec<ObservationDeletion>) -> ApiResult<()> {
        crate::backend::turso::db::delete_observations(&self.conn, deletions)
            .await
            .map_err(be)
    }

    async fn delete_observation_by_id(&self, entity_name: &str, id: i64) -> ApiResult<()> {
        crate::backend::turso::db::delete_observation_by_id(&self.conn, entity_name, id)
            .await
            .map_err(be)
    }

    async fn update_observation_by_id(
        &self,
        entity_name: &str,
        id: i64,
        new_content: &str,
    ) -> ApiResult<()> {
        crate::backend::turso::db::update_observation_by_id(
            &self.conn,
            entity_name,
            id,
            new_content,
        )
        .await
        .map_err(be)
    }

    async fn update_observation(
        &self,
        entity_name: &str,
        old_content: &str,
        new_content: &str,
    ) -> ApiResult<()> {
        crate::backend::turso::db::update_observation(
            &self.conn,
            entity_name,
            old_content,
            new_content,
        )
        .await
        .map_err(be)
    }

    async fn delete_relations(&self, relations: Vec<RelationInput>) -> ApiResult<()> {
        crate::backend::turso::db::delete_relations(&self.conn, relations)
            .await
            .map_err(be)
    }

    async fn truth_upsert(&self, entity: &str, key: &str, value: &str) -> ApiResult<()> {
        crate::backend::turso::db::truth_upsert(&self.conn, entity, key, value)
            .await
            .map_err(be)
    }

    async fn truth_delete(&self, entity: &str, key: &str) -> ApiResult<()> {
        crate::backend::turso::db::truth_delete(&self.conn, entity, key)
            .await
            .map_err(be)
    }

    async fn read_graph(&self) -> ApiResult<Graph> {
        crate::backend::turso::db::read_graph(&self.conn)
            .await
            .map_err(be)
    }

    async fn read_graph_full(&self) -> ApiResult<Graph> {
        crate::backend::turso::db::read_graph_eager(&self.conn)
            .await
            .map_err(be)
    }

    async fn read_graph_scoped(&self, scope: &[String], rationale: bool) -> ApiResult<Graph> {
        crate::backend::turso::db::read_graph_scoped(&self.conn, scope, rationale)
            .await
            .map_err(be)
    }

    async fn open_nodes(&self, req: OpenNodes) -> ApiResult<Graph> {
        crate::backend::turso::db::open_nodes_detailed(
            &self.conn,
            req.names,
            req.with_ids,
            &req.expand,
        )
        .await
        .map_err(be)
    }
}

impl SearchStore for TursoBackend {
    async fn search_nodes(&self, query: SearchQuery) -> ApiResult<Graph> {
        crate::backend::turso::db::search_nodes_with_limit(
            &self.conn,
            &query.query,
            query.limit,
            &query.filters,
        )
        .await
        .map_err(be)
    }

    async fn search_topics(&self, query: &str, limit: usize) -> ApiResult<Vec<SearchResult>> {
        let rows = crate::backend::turso::db::search_fts(&self.conn, query, limit)
            .await
            .map_err(be)?;
        Ok(rows
            .into_iter()
            .map(|(id, title, file_path, score)| SearchResult {
                id,
                title,
                file_path,
                score,
            })
            .collect())
    }

    async fn topics_by_id(&self, ids: &[String]) -> ApiResult<Vec<SearchResult>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (1..=ids.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT id, title, file_path FROM topics WHERE id IN ({placeholders})");
        let params = ids
            .iter()
            .cloned()
            .map(turso::Value::from)
            .collect::<Vec<_>>();
        let mut rows = self
            .conn
            .query(&sql, turso::params_from_iter(params))
            .await
            .map_err(|error| ApiError::Backend(error.to_string()))?;
        let mut results = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|error| ApiError::Backend(error.to_string()))?
        {
            results.push(SearchResult {
                id: row
                    .get(0)
                    .map_err(|error| ApiError::Backend(error.to_string()))?,
                title: row
                    .get(1)
                    .map_err(|error| ApiError::Backend(error.to_string()))?,
                file_path: row
                    .get(2)
                    .map_err(|error| ApiError::Backend(error.to_string()))?,
                score: 0.0,
            });
        }
        Ok(results)
    }
}

impl MaintenanceStore for TursoBackend {
    async fn stats(&self) -> ApiResult<Stats> {
        let (entities, relations, observations) = crate::backend::turso::db::stats(&self.conn)
            .await
            .map_err(be)?;
        Ok(Stats {
            entities,
            relations,
            observations,
        })
    }

    async fn stats_per_entity(&self) -> ApiResult<Vec<(String, usize)>> {
        crate::backend::turso::db::stats_per_entity(&self.conn)
            .await
            .map_err(be)
    }

    async fn reset(&self) -> ApiResult<()> {
        crate::backend::turso::db::reset(&self.conn)
            .await
            .map_err(be)
    }

    async fn capabilities(&self) -> ApiResult<BackendCapabilities> {
        // Document/vector operations are available when the optional document
        // tier is compiled; snapshot support remains a later task.
        Ok(BackendCapabilities {
            backend: "turso".to_string(),
            keyword_search: true,
            documents: cfg!(feature = "documents"),
            vectors: cfg!(feature = "documents"),
            logical_snapshots: false,
            file_backup: true,
        })
    }

    async fn health(&self) -> ApiResult<BackendHealth> {
        let reachable = self.conn.query("SELECT 1", ()).await.is_ok();
        Ok(BackendHealth {
            backend: "turso".to_string(),
            reachable,
            detail: None,
        })
    }
}

// Documents/vector tier — implemented in v0.5 task-4 (turso vector32 +
// vector_distance_cos). Until then the backend reports the capability as absent
// and rejects the calls explicitly rather than silently no-op'ing.
impl DocumentStore for TursoBackend {
    async fn upsert_topic(&self, topic: TopicSnapshot) -> ApiResult<()> {
        #[cfg(feature = "documents")]
        {
            return crate::backend::turso::db::upsert_topic(
                &self.conn,
                &topic.id,
                &topic.title,
                &topic.file_path,
                &topic.body,
            )
            .await
            .map_err(be);
        }
        #[cfg(not(feature = "documents"))]
        {
            let _ = topic;
            Err(ApiError::Unsupported("documents"))
        }
    }

    async fn delete_topic(&self, id: &str) -> ApiResult<()> {
        #[cfg(feature = "documents")]
        {
            return crate::backend::turso::db::delete_topic(&self.conn, id)
                .await
                .map_err(be);
        }
        #[cfg(not(feature = "documents"))]
        {
            let _ = id;
            Err(ApiError::Unsupported("documents"))
        }
    }

    async fn insert_chunks(&self, chunks: Vec<DocumentChunk>) -> ApiResult<()> {
        #[cfg(feature = "documents")]
        {
            let chunks = chunks
                .into_iter()
                .map(|chunk| crate::backend::turso::vector::Chunk {
                    id: chunk.id,
                    topic_id: chunk.topic_id,
                    chunk_idx: chunk.chunk_idx,
                    text: chunk.text,
                    source: chunk.source,
                    vector: chunk.embedding,
                })
                .collect();
            return crate::backend::turso::vector::VectorStore::new(self.conn.clone())
                .insert_chunks(chunks)
                .await
                .map_err(be);
        }
        #[cfg(not(feature = "documents"))]
        {
            let _ = chunks;
            Err(ApiError::Unsupported("vectors"))
        }
    }

    async fn search_chunks(
        &self,
        embedding: &[f32],
        limit: usize,
    ) -> ApiResult<Vec<DocumentSearchResult>> {
        #[cfg(feature = "documents")]
        {
            let results = crate::backend::turso::vector::VectorStore::new(self.conn.clone())
                .search(embedding, limit)
                .await
                .map_err(be)?;
            return Ok(results
                .into_iter()
                .map(|result| DocumentSearchResult {
                    id: result.id,
                    topic_id: result.topic_id,
                    text: result.text,
                    source: result.source,
                    score: result.score,
                })
                .collect());
        }
        #[cfg(not(feature = "documents"))]
        {
            let _ = (embedding, limit);
            Err(ApiError::Unsupported("vectors"))
        }
    }

    async fn delete_chunks_by_topic(&self, topic_id: &str) -> ApiResult<()> {
        #[cfg(feature = "documents")]
        {
            return crate::backend::turso::vector::VectorStore::new(self.conn.clone())
                .delete_by_topic(topic_id)
                .await
                .map_err(be);
        }
        #[cfg(not(feature = "documents"))]
        {
            let _ = topic_id;
            Err(ApiError::Unsupported("vectors"))
        }
    }
}

// Logical snapshot — implemented once the document tier lands so a snapshot can
// carry topics + chunks. Graph-only snapshots can be added first if needed.
impl SnapshotStore for TursoBackend {
    async fn export_snapshot(&self) -> ApiResult<Snapshot> {
        Err(ApiError::Unsupported("snapshot"))
    }

    async fn import_snapshot(&self, _snapshot: Snapshot) -> ApiResult<()> {
        Err(ApiError::Unsupported("snapshot"))
    }
}
