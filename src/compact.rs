use crate::db;
use crate::ingest::ingest_file;
use crate::vector::VectorStore;
use anyhow::Result;
use libsql::Connection;
use slug::slugify;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub async fn sync_graph_to_markdown(
    conn: &Connection,
    store: &VectorStore,
    embedder: &dyn crate::embed::EmbeddingProvider,
) -> Result<usize> {
    let paths = crate::paths::RosemaryPaths::resolve();
    let topics_dir = paths.topics_dir;
    if !topics_dir.exists() {
        std::fs::create_dir_all(&topics_dir)?;
    }

    let graph = db::mcp_read_graph(conn).await?;
    let mut count = 0;

    for entity in graph.entities {
        let slug = slugify(&entity.name);
        let file_path = topics_dir.join(format!("{}.md", slug));

        let mut content = String::new();
        content.push_str("---\n");
        content.push_str(&format!("title: {}\n", entity.name));
        content.push_str(&format!("type: {}\n", entity.entity_type));
        content.push_str(&format!("slug: {}\n", slug));
        content.push_str("---\n\n");

        if !entity.observations.is_empty() {
            content.push_str("## Observations\n\n");
            let mut unique_obs: Vec<String> = entity
                .observations
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            unique_obs.sort();
            for obs in unique_obs {
                content.push_str(&format!("* {}\n", obs));
            }
            content.push('\n');
        }

        let relations: Vec<_> = graph
            .relations
            .iter()
            .filter(|r| r.from == entity.name || r.to == entity.name)
            .collect();

        if !relations.is_empty() {
            content.push_str("## Relations\n\n");
            for rel in relations {
                if rel.from == entity.name {
                    content.push_str(&format!(
                        "* {} [[{}]]\n",
                        rel.relation_type,
                        slugify(&rel.to)
                    ));
                } else {
                    content.push_str(&format!(
                        "* [[{}]] is {} of this\n",
                        slugify(&rel.from),
                        rel.relation_type
                    ));
                }
            }
        }

        let mut file = std::fs::File::create(&file_path)?;
        file.write_all(content.as_bytes())?;

        // Re-ingest to update Vector/FTS tier
        ingest_file(&file_path, conn, store, embedder).await?;
        count += 1;
    }

    Ok(count)
}

pub fn prune_old_sessions(kb_root: &str, days: u32) -> Result<usize> {
    let sessions_dir = PathBuf::from(kb_root).join("sessions");
    if !sessions_dir.exists() {
        return Ok(0);
    }

    let cutoff = SystemTime::now() - Duration::from_secs(days as u64 * 86400);
    let mut pruned = 0;

    for entry in std::fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.modified()? < cutoff {
            std::fs::remove_file(path)?;
            pruned += 1;
        }
    }
    Ok(pruned)
}

/// Fetch all topic title embeddings and cluster by similarity.
/// Returns clusters of topic IDs with pairwise cosine similarity > threshold.
pub async fn find_duplicate_clusters(
    store: &crate::vector::VectorStore,
    conn: &libsql::Connection,
    threshold: f32,
) -> anyhow::Result<Vec<Vec<String>>> {
    use futures::StreamExt;
    use lancedb::query::{ExecutableQuery, QueryBase};

    // Get all topic IDs
    let mut rows = conn.query("SELECT id FROM topics", ()).await?;
    let mut topic_ids = Vec::new();
    while let Some(row) = rows.next().await? {
        topic_ids.push(row.get::<String>(0)?);
    }

    let mut clusters: Vec<Vec<String>> = Vec::new();
    let mut clustered: std::collections::HashSet<String> = std::collections::HashSet::new();

    for id in &topic_ids {
        if clustered.contains(id) {
            continue;
        }

        // Fetch representative vector for this topic (first chunk)
        let table = store.db().open_table("chunks").execute().await?;
        let mut results = table
            .query()
            .only_if(format!("topic_id = '{}'", id.replace('\'', "''")))
            .limit(1)
            .execute()
            .await?;

        if let Some(batch) = results.next().await {
            let batch = batch?;
            if batch.num_rows() > 0 {
                let vectors = batch
                    .column_by_name("vector")
                    .unwrap()
                    .as_any()
                    .downcast_ref::<arrow_array::FixedSizeListArray>()
                    .unwrap();
                let vector_val = vectors.value(0);
                let vector_data = vector_val
                    .as_any()
                    .downcast_ref::<arrow_array::Float32Array>()
                    .unwrap();
                let vector: Vec<f32> = vector_data.values().to_vec();

                let similar = store.search(&vector, 10).await?;
                let mut cluster = vec![id.clone()];
                for s in similar {
                    if s.score >= threshold && s.topic_id != *id && !clustered.contains(&s.topic_id)
                    {
                        cluster.push(s.topic_id.clone());
                        clustered.insert(s.topic_id.clone());
                    }
                }

                if cluster.len() > 1 {
                    clustered.insert(id.clone());
                    clusters.push(cluster);
                }
            }
        }
    }
    Ok(clusters)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_prune_removes_old_session_files() {
        let dir = tempdir().unwrap();
        let sessions_dir = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Create a file with old mtime
        let old_path = sessions_dir.join("2020-01-01-0000.md");
        std::fs::File::create(&old_path)
            .unwrap()
            .write_all(b"old")
            .unwrap();

        // Set mtime to 2020-01-01
        let old_time = SystemTime::UNIX_EPOCH + Duration::from_secs(1577836800);
        filetime::set_file_mtime(&old_path, filetime::FileTime::from_system_time(old_time))
            .unwrap();

        let count = prune_old_sessions(dir.path().to_str().unwrap(), 90).unwrap();
        assert_eq!(count, 1);
        assert!(!old_path.exists());
    }
}
