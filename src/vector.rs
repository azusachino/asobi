use anyhow::Result;
use arrow_array::builder::FixedSizeListBuilder;
use arrow_array::{Float32Array, RecordBatch, RecordBatchIterator, StringArray, UInt32Array};
use arrow_schema::{DataType, Field, Schema};
use futures::StreamExt;
use lancedb::connect;
use lancedb::query::{ExecutableQuery, QueryBase};
use std::sync::Arc;

const TABLE: &str = "chunks";

pub struct VectorStore {
    db: lancedb::Connection,
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
    pub async fn new(path: &str) -> Result<Self> {
        // Default dim=384 for all-MiniLML6V2; can be overridden
        Self::new_with_dim(path, 384).await
    }

    pub async fn new_with_dim(path: &str, dim: usize) -> Result<Self> {
        let db = connect(path).execute().await?;
        // Create table if absent by inserting an empty batch
        if !db
            .table_names()
            .execute()
            .await?
            .contains(&TABLE.to_string())
        {
            let schema = Arc::new(Self::schema(dim));
            let batch = RecordBatch::new_empty(schema.clone());
            db.create_table(TABLE, RecordBatchIterator::new(vec![Ok(batch)], schema))
                .execute()
                .await?;
        }
        Ok(Self { db, dim })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    pub fn db(&self) -> &lancedb::Connection {
        &self.db
    }

    fn schema(dim: usize) -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("topic_id", DataType::Utf8, false),
            Field::new("chunk_idx", DataType::UInt32, false),
            Field::new("text", DataType::Utf8, false),
            Field::new("source", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dim as i32,
                ),
                true,
            ),
        ])
    }

    pub async fn insert_chunks(&self, chunks: Vec<Chunk>) -> Result<()> {
        if chunks.is_empty() {
            return Ok(());
        }
        let dim = self.dim;
        let schema = Arc::new(Self::schema(dim));

        let ids = StringArray::from(chunks.iter().map(|c| c.id.as_str()).collect::<Vec<_>>());
        let topic_ids = StringArray::from(
            chunks
                .iter()
                .map(|c| c.topic_id.as_str())
                .collect::<Vec<_>>(),
        );
        let idxs = UInt32Array::from(chunks.iter().map(|c| c.chunk_idx).collect::<Vec<_>>());
        let texts = StringArray::from(chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>());
        let sources =
            StringArray::from(chunks.iter().map(|c| c.source.as_str()).collect::<Vec<_>>());

        let mut vec_builder =
            FixedSizeListBuilder::new(arrow_array::builder::Float32Builder::new(), dim as i32);
        for c in &chunks {
            vec_builder.values().append_slice(&c.vector);
            vec_builder.append(true);
        }
        let vectors = Arc::new(vec_builder.finish());

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(ids),
                Arc::new(topic_ids),
                Arc::new(idxs),
                Arc::new(texts),
                Arc::new(sources),
                vectors,
            ],
        )?;

        let table = self.db.open_table(TABLE).execute().await?;
        table
            .add(RecordBatchIterator::new(vec![Ok(batch)], schema))
            .execute()
            .await?;
        Ok(())
    }

    pub async fn search(&self, vector: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        let table = self.db.open_table(TABLE).execute().await?;
        let mut results = table.vector_search(vector)?.limit(limit).execute().await?;

        let mut out = Vec::new();
        while let Some(batch) = results.next().await {
            let batch = batch?;
            let ids = batch
                .column_by_name("id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let topic_ids = batch
                .column_by_name("topic_id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let texts = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let sources = batch
                .column_by_name("source")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let scores = batch
                .column_by_name("_distance")
                .unwrap()
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();

            for i in 0..batch.num_rows() {
                out.push(SearchResult {
                    id: ids.value(i).to_string(),
                    topic_id: topic_ids.value(i).to_string(),
                    text: texts.value(i).to_string(),
                    source: sources.value(i).to_string(),
                    score: 1.0 - scores.value(i), // convert distance → similarity
                });
            }
        }
        Ok(out)
    }

    pub async fn delete_by_topic(&self, topic_id: &str) -> Result<()> {
        let table = self.db.open_table(TABLE).execute().await?;
        let escaped_id = topic_id.replace('\'', "''");
        table
            .delete(&format!("topic_id = '{}'", escaped_id))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_chunk(i: u32, dim: usize) -> Chunk {
        Chunk {
            id: format!("chunk-{}", i),
            topic_id: "topic-1".into(),
            chunk_idx: i,
            text: format!("chunk text {}", i),
            source: ".rosemary/topics/test.md".into(),
            vector: vec![i as f32 / 10.0; dim],
        }
    }

    #[tokio::test]
    async fn test_insert_and_search() {
        let dir = tempdir().unwrap();
        let store = VectorStore::new_with_dim(dir.path().to_str().unwrap(), 4)
            .await
            .unwrap();

        let dim = 4;
        let chunks: Vec<Chunk> = (0..5).map(|i| make_chunk(i, dim)).collect();
        store.insert_chunks(chunks).await.unwrap();

        let query = vec![0.0f32; dim];
        let results = store.search(&query, 3).await.unwrap();
        assert_eq!(results.len(), 3);
    }

    #[tokio::test]
    async fn test_delete_by_topic() {
        let dir = tempdir().unwrap();
        let store = VectorStore::new_with_dim(dir.path().to_str().unwrap(), 4)
            .await
            .unwrap();
        let dim = 4;
        store.insert_chunks(vec![make_chunk(0, dim)]).await.unwrap();
        store.delete_by_topic("topic-1").await.unwrap();
        let results = store.search(&vec![0.0f32; dim], 10).await.unwrap();
        assert_eq!(results.len(), 0);
    }
}
