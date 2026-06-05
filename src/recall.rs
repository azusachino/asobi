use crate::{db::search_fts, embed::EmbeddingProvider, vector::VectorStore};
use anyhow::Result;
use libsql::Connection;
use std::collections::HashMap;

#[derive(Debug)]
pub struct RecallResult {
    pub topic_id: String,
    pub title: String,
    pub file_path: String,
    pub snippet: String,
    pub score: f32,
}

pub async fn recall(
    query: &str,
    conn: &Connection,
    store: &VectorStore,
    embedder: &impl EmbeddingProvider,
    top_k: usize,
) -> Result<Vec<RecallResult>> {
    // --- ANN search (weight 0.7) ---
    let query_vec = embedder.embed(&[query.to_string()]).await?;
    let ann_results = store.search(&query_vec[0], top_k * 4).await?;

    // --- FTS5 keyword search (weight 0.3) ---
    // Escape FTS5 special chars before querying
    let safe_query = query.replace('"', "\"\"");
    let fts_results = search_fts(conn, &format!("\"{}\"", safe_query), top_k * 2)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("FTS5 search failed, falling back to ANN-only: {e}");
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

    // FTS5 bm25 scores are negative in SQLite (lower = better match)
    let fts_max = fts_results.iter().map(|r| r.3.abs()).fold(0.0f64, f64::max);
    for (id, title, path, bm25) in &fts_results {
        let norm_score = if fts_max > 0.0 {
            (bm25.abs() / fts_max) as f32
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

    // Fill in title/path for ANN-only hits from DB
    for (topic_id, (_, _, _, title, path)) in scores.iter_mut() {
        if title.is_empty() {
            let mut rows = conn
                .query(
                    "SELECT title, file_path FROM topics WHERE id=?1",
                    libsql::params![topic_id.clone()],
                )
                .await?;
            if let Some(row) = rows.next().await? {
                *title = row.get(0)?;
                *path = row.get(1)?;
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
    use crate::{db::init_db, ingest::ingest_file, vector::VectorStore};
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
            std::env::set_var(crate::db::ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        let mut f = std::fs::File::create(dir.path().join("rust-pinning.md")).unwrap();
        writeln!(f, "---\ntitle: Rust Pinning\nslug: rust-pinning\n---\n\nPinning is a mechanism to prevent moves.").unwrap();

        let (_db, conn) = init_db().await.unwrap();
        let store = VectorStore::new_with_dim(conn.clone(), 384);
        let embedder = FakeEmbedder(384);

        ingest_file(
            dir.path().join("rust-pinning.md").as_path(),
            &conn,
            &store,
            &embedder,
        )
        .await
        .unwrap();

        let results = recall("pinning", &conn, &store, &embedder, 5)
            .await
            .unwrap();
        assert!(!results.is_empty(), "expected at least one result");
        assert!(results[0].title.contains("Pinning"));
    }
}
