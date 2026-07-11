use crate::{api::v1::DocumentStore, embed::EmbeddingProvider};
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
    let fts_results = document_store
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
        for result in document_store.topics_by_id(chunk).await? {
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
    use crate::{ingest::ingest_file, storage::Storage};
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
            std::env::set_var(crate::paths::ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        let mut f = std::fs::File::create(dir.path().join("rust-pinning.md")).unwrap();
        writeln!(f, "---\ntitle: Rust Pinning\nslug: rust-pinning\n---\n\nPinning is a mechanism to prevent moves.").unwrap();

        let storage = Storage::open_default().await.unwrap();
        let embedder = FakeEmbedder(768);

        ingest_file(
            dir.path().join("rust-pinning.md").as_path(),
            &storage,
            &embedder,
        )
        .await
        .unwrap();

        let results = recall("pinning", &storage, &embedder, 5).await.unwrap();
        assert!(!results.is_empty(), "expected at least one result");
        assert!(results[0].title.contains("Pinning"));
    }

    // End-to-end proof that the real embedding model does semantic recall, not
    // just keyword matching. Each query below shares NO salient words with its
    // target document, so only a working embedding model can rank it first. This
    // is the release gate for any future model swap — it downloads the model
    // once into a stable cache and reuses it on subsequent runs.
    #[tokio::test]
    async fn test_recall_ranks_paraphrase_with_real_model() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("real_model.db");
        unsafe {
            std::env::set_var(crate::paths::ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        // Shared, stable weight cache: downloaded once per machine, reused across
        // runs and across the other document-tier tests.
        let cache_dir = std::env::temp_dir().join("asobi-fastembed-test-cache");

        let docs = [
            (
                "storage-concurrency.md",
                "Storage Concurrency",
                "The libSQL backend coordinates parallel writers using write-ahead \
                 logging. When several processes modify the database at the same \
                 moment, a bounded retry loop absorbs transient lock contention.",
            ),
            (
                "network-resilience.md",
                "Network Resilience",
                "Outbound HTTP requests use exponential backoff with jitter. A failed \
                 call is retried a few times before surfacing an error, smoothing over \
                 brief upstream outages and rate-limit responses.",
            ),
            (
                "semantic-recall.md",
                "Semantic Recall",
                "Ingested Markdown is chunked and each chunk is turned into a dense \
                 vector by an embedding model. Queries are embedded the same way and \
                 matched by cosine distance.",
            ),
        ];
        for (file, title, body) in &docs {
            let mut f = std::fs::File::create(dir.path().join(file)).unwrap();
            writeln!(f, "---\ntitle: {title}\n---\n\n{body}").unwrap();
        }

        let storage = Storage::open_default().await.unwrap();
        let embedder = crate::embed::FastEmbedProvider::new(cache_dir).unwrap();
        for (file, _, _) in &docs {
            ingest_file(dir.path().join(file).as_path(), &storage, &embedder)
                .await
                .unwrap();
        }

        // (paraphrase query with no shared keywords, expected top title)
        let cases = [
            (
                "how do I stop simultaneous updates from clobbering each other",
                "Storage Concurrency",
            ),
            (
                "recovering from a flaky remote service that times out",
                "Network Resilience",
            ),
            (
                "finding documents by meaning instead of exact wording",
                "Semantic Recall",
            ),
        ];
        for (query, expected_title) in cases {
            let results = recall(query, &storage, &embedder, 3).await.unwrap();
            assert!(!results.is_empty(), "no results for query: {query}");
            assert_eq!(
                results[0].title,
                expected_title,
                "query {query:?} should rank {expected_title:?} first, got {:?} (scores: {:?})",
                results[0].title,
                results
                    .iter()
                    .map(|r| (&r.title, r.score))
                    .collect::<Vec<_>>(),
            );
            if results.len() > 1 {
                assert!(
                    results[0].score > results[1].score,
                    "top score for {query:?} should beat the runner-up",
                );
            }
        }
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
