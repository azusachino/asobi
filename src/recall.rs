use crate::{
    api::v1::{DocumentStore, SearchStore},
    embed::EmbeddingProvider,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecallResult {
    pub topic_id: String,
    pub title: String,
    pub file_path: String,
    pub snippet: String,
    pub score: f32,
}

pub async fn recall(
    query: &str,
    document_store: &impl DocumentStore,
    search_store: &impl SearchStore,
    embedder: &impl EmbeddingProvider,
    top_k: usize,
) -> Result<Vec<RecallResult>> {
    // --- ANN search (weight 0.7) ---
    let query_vec = embedder.embed(&[query.to_string()]).await?;
    let ann_results = document_store
        .search_chunks(&query_vec[0], top_k * 4)
        .await?;

    // --- Turso FTS keyword search (weight 0.3) ---
    let safe_query = query.replace('"', "\"\"");
    let fts_results = search_store
        .search_topics(&format!("\"{}\"", safe_query), top_k * 2)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("Turso FTS search failed, falling back to ANN-only: {e}");
            vec![]
        });

    // --- Merge by topic_id, pick best snippet, combine scores ---
    // topic_id -> (score, snippet, snippet_score, title, path)
    let mut scores: HashMap<String, (f32, String, f32, String, String)> = HashMap::new();

    for r in &ann_results {
        let entry = scores.entry(r.topic_id.clone()).or_insert((
            0.0,
            String::new(),
            -1.0,
            String::new(),
            String::new(),
        ));
        entry.0 += r.score * 0.7;
        // Keep the snippet with the highest individual score
        if r.score > entry.2 {
            entry.1 = r.text.clone();
            entry.2 = r.score;
        }
    }

    let fts_max = fts_results.iter().map(|r| r.score).fold(0.0f64, f64::max);
    for result in &fts_results {
        let id = &result.id;
        let title = &result.title;
        let path = &result.file_path;
        let norm_score = if fts_max > 0.0 {
            (result.score / fts_max) as f32
        } else {
            0.0
        };
        let entry = scores.entry(id.clone()).or_insert((
            0.0,
            String::new(),
            -1.0,
            title.clone(),
            path.clone(),
        ));
        entry.0 += norm_score * 0.3;
        if entry.3.is_empty() {
            entry.3 = title.clone();
        }
        if entry.4.is_empty() {
            entry.4 = path.clone();
        }
    }

    // Fill in title/path for ANN-only hits from DB in batched queries.
    let missing_ids: Vec<String> = scores
        .iter()
        .filter(|(_, (_, _, _, title, _))| title.is_empty())
        .map(|(topic_id, _)| topic_id.clone())
        .collect();
    for chunk in missing_ids.chunks(500) {
        for result in search_store.topics_by_id(chunk).await? {
            if let Some((_, _, _, title, path)) = scores.get_mut(&result.id) {
                *title = result.title;
                *path = result.file_path;
            }
        }
    }

    let mut ranked: Vec<RecallResult> = scores
        .into_iter()
        .map(
            |(topic_id, (score, snippet, _, title, file_path))| RecallResult {
                topic_id,
                title,
                file_path,
                snippet,
                score,
            },
        )
        .collect();

    ranked.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    ranked.truncate(top_k);
    Ok(ranked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend::turso::db::init_db, ingest::ingest_file};
    use std::io::Write;
    use tempfile::tempdir;

    struct FakeEmbedder(usize);
    impl EmbeddingProvider for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            // Make "pinning" queries return a distinctive vector
            Ok(texts
                .iter()
                .map(|t| {
                    if t.contains("pinning") {
                        vec![1.0f32; self.0]
                    } else {
                        vec![0.0f32; self.0]
                    }
                })
                .collect())
        }
        fn dim(&self) -> usize {
            self.0
        }
    }

    #[tokio::test]
    async fn test_recall_returns_relevant_topic() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var(
                crate::backend::turso::db::ENV_DATABASE_URL,
                db_path.to_str().unwrap(),
            );
        }

        let mut f = std::fs::File::create(dir.path().join("rust-pinning.md")).unwrap();
        writeln!(f, "---\ntitle: Rust Pinning\nslug: rust-pinning\n---\n\nPinning is a mechanism to prevent moves.").unwrap();

        let (db, conn) = init_db().await.unwrap();
        let backend = crate::backend::TursoBackend::from_parts(db, conn);
        let embedder = FakeEmbedder(384);

        ingest_file(
            dir.path().join("rust-pinning.md").as_path(),
            &backend,
            &embedder,
        )
        .await
        .unwrap();

        let results = recall("pinning", &backend, &backend, &embedder, 5)
            .await
            .unwrap();
        assert!(!results.is_empty(), "expected at least one result");
        assert!(results[0].title.contains("Pinning"));
    }

    #[test]
    fn test_recall_result_serialization() {
        let result = RecallResult {
            topic_id: "test-id".to_string(),
            title: "Test Title".to_string(),
            file_path: "path/to/file.md".to_string(),
            snippet: "some snippet text".to_string(),
            score: 0.85,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"topicId\":\"test-id\""));
        assert!(json.contains("\"title\":\"Test Title\""));
        assert!(json.contains("\"filePath\":\"path/to/file.md\""));
        assert!(json.contains("\"snippet\":\"some snippet text\""));
        assert!(json.contains("\"score\":0.85"));

        let deserialized: RecallResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.topic_id, "test-id");
        assert_eq!(deserialized.title, "Test Title");
        assert_eq!(deserialized.file_path, "path/to/file.md");
        assert_eq!(deserialized.snippet, "some snippet text");
        assert_eq!(deserialized.score, 0.85);
    }
}
