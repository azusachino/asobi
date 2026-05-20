use crate::{
    chunk::chunk_text,
    db::upsert_topic,
    embed::EmbeddingProvider,
    vector::{Chunk, VectorStore},
};
use anyhow::Result;
use libsql::Connection;
use slug::slugify;
use std::path::Path;
use uuid::Uuid;
use walkdir::WalkDir;

pub async fn ingest_file(
    path: &Path,
    conn: &Connection,
    store: &VectorStore,
    embedder: &dyn EmbeddingProvider,
) -> Result<()> {
    let raw = std::fs::read_to_string(path)?;

    // Parse optional YAML frontmatter (between --- delimiters)
    let (mut title, body) = parse_frontmatter(&raw);

    // Fallback to filename stem if title is still Untitled
    if title == "Untitled"
        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
    {
        title = stem.replace(['-', '_'], " ");
        // capitalise words
        title = title
            .split_whitespace()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }

    let slug = slugify(&title);
    let file_path = path.to_str().unwrap_or_default().to_string();

    // Delete old chunks for this topic before re-indexing
    store.delete_by_topic(&slug).await?;

    // Chunk and embed
    let texts = chunk_text(&body, 512, 64);
    if texts.is_empty() {
        upsert_topic(conn, &slug, &title, &file_path, &body).await?;
        return Ok(());
    }

    let vectors = embedder.embed(&texts).await?;
    let chunks: Vec<Chunk> = texts
        .into_iter()
        .zip(vectors)
        .enumerate()
        .map(|(i, (text, vector))| Chunk {
            id: Uuid::new_v4().to_string(),
            topic_id: slug.clone(),
            chunk_idx: i as u32,
            text,
            source: file_path.clone(),
            vector,
        })
        .collect();

    store.insert_chunks(chunks).await?;
    upsert_topic(conn, &slug, &title, &file_path, &body).await?;
    Ok(())
}

pub async fn ingest_dir(
    dir: &Path,
    conn: &Connection,
    store: &VectorStore,
    embedder: &dyn EmbeddingProvider,
) -> Result<usize> {
    let mut count = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.path().extension().and_then(|s| s.to_str()) == Some("md") {
            ingest_file(entry.path(), conn, store, embedder).await?;
            count += 1;
        }
    }
    Ok(count)
}

/// Returns (title, body). Title comes from frontmatter `title:` if present,
/// otherwise the filename stem.
fn parse_frontmatter(raw: &str) -> (String, String) {
    if let Some(rest) = raw.strip_prefix("---")
        && let Some(end) = rest.find("\n---")
    {
        let fm = &rest[..end];
        let body = rest[end + 4..].trim_start_matches('\n').to_string();
        let title = fm
            .lines()
            .find(|l| l.starts_with("title:"))
            .map(|l| l.trim_start_matches("title:").trim().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        return (title, body);
    }

    // Try to find "title:" in the first few lines (legacy format)
    for line in raw.lines().take(5) {
        if let Some(idx) = line.find("title:") {
            let rest = &line[idx + 6..];
            let title_end = rest.find("slug:").unwrap_or(rest.len());
            let title = rest[..title_end].trim().to_string();
            if !title.is_empty() {
                return (title, raw.to_string());
            }
        }
    }

    ("Untitled".to_string(), raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use crate::vector::VectorStore;
    use async_trait::async_trait;
    use std::io::Write;
    use tempfile::tempdir;

    struct FakeEmbedder(usize);
    #[async_trait]
    impl EmbeddingProvider for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.1f32; self.0]).collect())
        }
        fn dim(&self) -> usize {
            self.0
        }
    }

    #[tokio::test]
    async fn test_ingest_single_file() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
        }

        let mut f = std::fs::File::create(dir.path().join("rust-pinning.md")).unwrap();
        writeln!(
            f,
            "---\ntitle: Rust Pinning\nslug: rust-pinning\n---\n\nPinning is a mechanism..."
        )
        .unwrap();

        let (_db, conn) = init_db().await.unwrap();
        let store = VectorStore::new_with_dim(dir.path().join("lance").to_str().unwrap(), 4)
            .await
            .unwrap();
        let embedder = FakeEmbedder(4);

        ingest_file(
            dir.path().join("rust-pinning.md").as_path(),
            &conn,
            &store,
            &embedder,
        )
        .await
        .unwrap();

        let results = crate::db::search_fts(&conn, "pinning", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1, "Rust Pinning");
    }

    #[tokio::test]
    async fn test_ingest_dir_counts_files() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test2.db");
        unsafe {
            std::env::set_var("DATABASE_URL", db_path.to_str().unwrap());
        }

        for name in &["a.md", "b.md", "c.md"] {
            let mut f = std::fs::File::create(dir.path().join(name)).unwrap();
            writeln!(
                f,
                "---\ntitle: {name}\nslug: {name}\n---\n\nContent of {name}."
            )
            .unwrap();
        }

        let (_db, conn) = init_db().await.unwrap();
        let store = VectorStore::new_with_dim(dir.path().join("lance2").to_str().unwrap(), 4)
            .await
            .unwrap();
        let embedder = FakeEmbedder(4);

        let count = ingest_dir(dir.path(), &conn, &store, &embedder)
            .await
            .unwrap();
        assert_eq!(count, 3);
    }
}
