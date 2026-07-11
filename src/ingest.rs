use crate::{
    api::v1::{DocumentChunk, DocumentStore, TopicSnapshot},
    chunk::chunk_text,
    embed::EmbeddingProvider,
    normalize::slugify,
};
use anyhow::Result;
use std::path::Path;
use uuid::Uuid;
use walkdir::WalkDir;

pub async fn ingest_file(
    path: &Path,
    store: &impl DocumentStore,
    embedder: &impl EmbeddingProvider,
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
    store.delete_chunks_by_topic(&slug).await?;

    // Chunk and embed
    let texts = chunk_text(&body, 512, 64);
    if texts.is_empty() {
        store
            .upsert_topic(TopicSnapshot {
                id: slug.clone(),
                title: title.clone(),
                file_path: file_path.clone(),
                body: body.clone(),
            })
            .await?;
        return Ok(());
    }

    let vectors = embedder.embed(&texts).await?;
    let chunks: Vec<DocumentChunk> = texts
        .into_iter()
        .zip(vectors)
        .enumerate()
        .map(|(i, (text, vector))| DocumentChunk {
            id: Uuid::now_v7().to_string(),
            topic_id: slug.clone(),
            chunk_idx: i as u32,
            text,
            source: file_path.clone(),
            embedding: vector,
        })
        .collect();

    store.insert_chunks(chunks).await?;
    store
        .upsert_topic(TopicSnapshot {
            id: slug,
            title,
            file_path,
            body,
        })
        .await?;
    Ok(())
}

pub async fn ingest_dir(
    dir: &Path,
    store: &impl DocumentStore,
    embedder: &impl EmbeddingProvider,
) -> Result<usize> {
    let mut count = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        if entry.path().extension().and_then(|s| s.to_str()) == Some("md") {
            ingest_file(entry.path(), store, embedder).await?;
            count += 1;
        }
    }
    Ok(count)
}

/// Returns (title, body). Title comes from frontmatter `title:` if present,
/// otherwise the filename stem.
fn parse_frontmatter(raw: &str) -> (String, String) {
    if let Some(fm) = crate::frontmatter::parse(raw) {
        let title = fm.get("title").unwrap_or("Untitled").to_string();
        return (title, fm.body);
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
    use crate::api::v1::DocumentStore;
    use crate::storage::Storage;
    use std::io::Write;
    use tempfile::tempdir;

    struct FakeEmbedder(usize);
    impl EmbeddingProvider for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            Ok(texts.iter().map(|_| vec![0.1f32; self.0]).collect())
        }
        fn dim(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn test_parse_frontmatter_unquotes_title_and_falls_back() {
        // Quoted title round-trips unquoted; a fenceless doc hits the legacy path.
        let (title, body) = parse_frontmatter("---\ntitle: \"asobi:session\"\n---\n\nBody line.\n");
        assert_eq!(title, "asobi:session");
        assert_eq!(body, "Body line.\n");

        let (legacy, _) = parse_frontmatter("title: Bare Title\nslug: x\n\nbody");
        assert_eq!(legacy, "Bare Title");
    }

    #[tokio::test]
    async fn test_ingest_single_file() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        unsafe {
            std::env::set_var(crate::paths::ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        let mut f = std::fs::File::create(dir.path().join("rust-pinning.md")).unwrap();
        writeln!(
            f,
            "---\ntitle: Rust Pinning\nslug: rust-pinning\n---\n\nPinning is a mechanism..."
        )
        .unwrap();

        let storage = Storage::open_default().await.unwrap();
        let embedder = FakeEmbedder(768);

        ingest_file(
            dir.path().join("rust-pinning.md").as_path(),
            &storage,
            &embedder,
        )
        .await
        .unwrap();

        let results = storage.search_topics("pinning", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Pinning");
    }

    #[tokio::test]
    async fn test_ingest_dir_counts_files() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test2.db");
        unsafe {
            std::env::set_var(crate::paths::ENV_DATABASE_URL, db_path.to_str().unwrap());
        }

        for name in &["a.md", "b.md", "c.md"] {
            let mut f = std::fs::File::create(dir.path().join(name)).unwrap();
            writeln!(
                f,
                "---\ntitle: {name}\nslug: {name}\n---\n\nContent of {name}."
            )
            .unwrap();
        }

        let storage = Storage::open_default().await.unwrap();
        let embedder = FakeEmbedder(768);

        let count = ingest_dir(dir.path(), &storage, &embedder).await.unwrap();
        assert_eq!(count, 3);
    }
}
