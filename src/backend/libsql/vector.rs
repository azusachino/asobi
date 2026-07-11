use anyhow::Result;
use libsql::{Connection, params};

pub struct VectorStore {
    conn: Connection,
    dim: usize,
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub id: String,
    pub topic_id: String,
    pub chunk_idx: u32,
    pub text: String,
    pub source: String,
    pub vector: Vec<f32>,
}

#[derive(Debug)]
pub struct SearchResult {
    pub id: String,
    pub topic_id: String,
    pub text: String,
    pub source: String,
    pub score: f32,
}

impl VectorStore {
    pub fn new(conn: Connection) -> Self {
        // Default dim=384 for all-MiniLML6V2
        Self::new_with_dim(conn, 384)
    }

    pub fn new_with_dim(conn: Connection, dim: usize) -> Self {
        Self { conn, dim }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub async fn insert_chunks(&self, chunks: Vec<Chunk>) -> Result<()> {
        crate::backend::libsql::tx::immediate_transaction(&self.conn, |tx| {
            let chunks = chunks.clone();
            Box::pin(async move {
                for chunk in chunks {
                    let vector_json = serde_json::to_string(&chunk.vector)
                        .map_err(|error| libsql::Error::Misuse(error.to_string()))?;
                    tx.execute(
                        crate::backend::libsql::constant::SQL_INSERT_CHUNK,
                        params![
                            chunk.id,
                            chunk.topic_id,
                            chunk.chunk_idx,
                            chunk.text,
                            chunk.source,
                            vector_json
                        ],
                    )
                    .await?;
                }
                Ok(())
            })
        })
        .await?;
        Ok(())
    }

    pub async fn search(&self, vector: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let vector_json = serde_json::to_string(vector)?;
        let mut rows = self
            .conn
            .query(
                crate::backend::libsql::constant::SQL_SEARCH_CHUNKS,
                params![vector_json, limit as i64],
            )
            .await?;

        let mut out = Vec::new();
        while let Some(row) = rows.next().await? {
            out.push(SearchResult {
                id: row.get(0)?,
                topic_id: row.get(1)?,
                text: row.get(2)?,
                source: row.get(3)?,
                score: 1.0 - row.get::<f64>(4)? as f32,
            });
        }
        Ok(out)
    }

    pub async fn delete_by_topic(&self, topic_id: &str) -> Result<()> {
        self.conn
            .execute(
                crate::backend::libsql::constant::SQL_DELETE_CHUNKS_BY_TOPIC,
                params![topic_id],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_test_db() -> Connection {
        let db = libsql::Builder::new_local(":memory:")
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute(
            "CREATE TABLE chunks (
                id        TEXT PRIMARY KEY,
                topic_id  TEXT NOT NULL,
                chunk_idx INTEGER NOT NULL,
                text      TEXT NOT NULL,
                source    TEXT NOT NULL,
                embedding F32_BLOB(384) NOT NULL
            )",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "CREATE INDEX idx_chunks_vector ON chunks(libsql_vector_idx(embedding, 'metric=cosine'))",
            (),
        )
        .await
        .unwrap();
        conn
    }

    fn make_chunk(i: u32, dim: usize) -> Chunk {
        Chunk {
            id: format!("chunk-{}", i),
            topic_id: "topic-1".into(),
            chunk_idx: i,
            text: format!("chunk text {}", i),
            source: ".asobi/topics/test.md".into(),
            vector: vec![i as f32 / 10.0; dim],
        }
    }

    #[tokio::test]
    async fn test_insert_and_search() {
        let conn = setup_test_db().await;
        let store = VectorStore::new_with_dim(conn, 384);

        let dim = 384;
        let chunks: Vec<Chunk> = (0..5).map(|i| make_chunk(i, dim)).collect();
        store.insert_chunks(chunks).await.unwrap();

        let query = vec![0.0f32; dim];
        let results = store.search(&query, 3).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_delete_by_topic() {
        let conn = setup_test_db().await;
        let store = VectorStore::new_with_dim(conn, 384);
        let dim = 384;
        store.insert_chunks(vec![make_chunk(0, dim)]).await.unwrap();
        store.delete_by_topic("topic-1").await.unwrap();
        let results = store.search(&vec![0.0f32; dim], 10).await.unwrap();
        assert_eq!(results.len(), 0);
    }

    #[tokio::test]
    async fn test_search_scores_by_similarity() {
        let conn = setup_test_db().await;
        let dim = 384;
        let store = VectorStore::new_with_dim(conn, dim);

        // chunk "a": aligned with first component [1, 0, 0, ...]
        let mut vec_a = vec![0.0f32; dim];
        vec_a[0] = 1.0;
        let chunk_a = Chunk {
            id: "chunk-a".into(),
            topic_id: "topic-a".into(),
            chunk_idx: 0,
            text: "chunk a text".into(),
            source: "test.md".into(),
            vector: vec_a,
        };

        // chunk "b": aligned with second component [0, 1, 0, ...]
        let mut vec_b = vec![0.0f32; dim];
        vec_b[1] = 1.0;
        let chunk_b = Chunk {
            id: "chunk-b".into(),
            topic_id: "topic-b".into(),
            chunk_idx: 0,
            text: "chunk b text".into(),
            source: "test.md".into(),
            vector: vec_b,
        };

        store.insert_chunks(vec![chunk_a, chunk_b]).await.unwrap();

        // query aligned to "a"
        let mut query = vec![0.0f32; dim];
        query[0] = 1.0;
        let results = store.search(&query, 2).await.unwrap();

        assert_eq!(results.len(), 2, "expected both chunks returned");

        // top result must be chunk-a (most similar to query)
        assert_eq!(results[0].id, "chunk-a", "chunk-a should rank first");

        // scores must not all be equal — real distance is being measured
        assert!(
            results[0].score > results[1].score,
            "top score ({}) should be strictly greater than second score ({})",
            results[0].score,
            results[1].score
        );
    }
}
