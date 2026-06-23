use crate::db;
use crate::ingest::ingest_file;
use crate::model::EntityOutput;
use crate::normalize::slugify;
use crate::vector::VectorStore;
use anyhow::Result;
use libsql::{Connection, params};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub async fn sync_graph_to_markdown(
    conn: &Connection,
    store: &VectorStore,
    embedder: &impl crate::embed::EmbeddingProvider,
) -> Result<usize> {
    let paths = crate::paths::AsobiPaths::resolve();
    let topics_dir = paths.topics_dir;
    if !topics_dir.exists() {
        std::fs::create_dir_all(&topics_dir)?;
    }

    let graph = db::read_graph_eager(conn).await?;
    let mut count = 0;

    for entity in &graph.entities {
        if !should_sync(&entity.entity_type) {
            continue;
        }

        let slug = slugify(&entity.name);
        let file_path = topics_dir.join(format!("{}.md", slug));
        let content = render_entity_markdown(entity, &slug, &graph.relations);

        std::fs::File::create(&file_path)?.write_all(content.as_bytes())?;

        // Re-ingest to update Vector/FTS tier
        ingest_file(&file_path, conn, store, embedder).await?;
        count += 1;
    }

    Ok(count)
}

/// The recall tier (Markdown topics + FTS/vector index) holds durable
/// *knowledge*, not volatile *state*. We skip:
/// - `skill`: the installer already chunks the full body into the document
///   tier under `topic_id = entity_name`; re-syncing here would emit a
///   body-less file under a different (slugified) topic id — a duplicate,
///   content-free topic.
/// - `session` / `task` (epics are `task` too): operational state that flips
///   constantly and is already cheaply queryable from the graph via
///   `search --where status=…` / `show`. Embedding it only churns the index
///   and pollutes semantic `query` results. Full archival lives in
///   `export` / `backup`, not here.
///
/// Denylist (not allowlist) so new knowledge types persist by default.
fn should_sync(entity_type: &str) -> bool {
    !matches!(entity_type, "skill" | "session" | "task")
}

/// Render one entity to its Markdown topic: frontmatter plus Truths
/// (current state), Observations (trail), and Relations sections.
fn render_entity_markdown(
    entity: &EntityOutput,
    slug: &str,
    relations: &[crate::model::RelationInput],
) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "---");
    let _ = writeln!(out, "title: {}", entity.name);
    let _ = writeln!(out, "type: {}", entity.entity_type);
    let _ = writeln!(out, "slug: {}", slug);
    let _ = writeln!(out, "---\n");

    if !entity.truths.is_empty() {
        let _ = writeln!(out, "## Truths\n");
        // BTreeMap iterates in key order, so output is deterministic.
        for (key, value) in &entity.truths {
            let _ = writeln!(out, "* {}: {}", key, value);
        }
        out.push('\n');
    }

    if !entity.observations.is_empty() {
        let _ = writeln!(out, "## Observations\n");
        let mut unique_obs: Vec<&String> = entity
            .observations
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        unique_obs.sort();
        for obs in unique_obs {
            let _ = writeln!(out, "* {}", obs);
        }
        out.push('\n');
    }

    let related: Vec<_> = relations
        .iter()
        .filter(|r| r.from == entity.name || r.to == entity.name)
        .collect();

    if !related.is_empty() {
        let _ = writeln!(out, "## Relations\n");
        for rel in related {
            if rel.from == entity.name {
                let _ = writeln!(out, "* {} [[{}]]", rel.relation_type, slugify(&rel.to));
            } else {
                let _ = writeln!(
                    out,
                    "* [[{}]] is {} of this",
                    slugify(&rel.from),
                    rel.relation_type
                );
            }
        }
    }

    out
}

pub fn prune_old_sessions(topics_root: &str, days: u32) -> Result<usize> {
    let sessions_dir = PathBuf::from(topics_root).join("sessions");
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
    // Get all topic IDs
    let mut rows = conn
        .query(crate::constant::SQL_SELECT_ALL_TOPIC_IDS, ())
        .await?;
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
        let mut rows = conn
            .query(
                "SELECT embedding FROM chunks WHERE topic_id = ?1 LIMIT 1",
                params![id.clone()],
            )
            .await?;

        if let Some(row) = rows.next().await? {
            let blob: Vec<u8> = row.get(0)?;
            // F32_BLOB is stored as little-endian f32s
            let vector: Vec<f32> = blob
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
                .collect();

            let similar = store.search(&vector, 10).await?;
            let mut cluster = vec![id.clone()];
            for s in similar {
                if s.score >= threshold && s.topic_id != *id && !clustered.contains(&s.topic_id) {
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

    #[test]
    fn test_should_sync_skips_volatile_and_skill_types() {
        // Knowledge → persisted to the recall tier.
        assert!(should_sync("project"));
        assert!(should_sync("concept"));
        assert!(should_sync("reference"));
        assert!(should_sync("preference"));
        assert!(should_sync("standard"));
        // Volatile state + self-indexing skills → graph-only.
        assert!(!should_sync("session"));
        assert!(!should_sync("task"));
        assert!(!should_sync("skill"));
    }

    fn entity(name: &str, entity_type: &str) -> EntityOutput {
        EntityOutput {
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            observations: Vec::new(),
            truths: std::collections::BTreeMap::new(),
            observation_count: 0,
            body: None,
        }
    }

    #[test]
    fn test_render_includes_truths_and_observations() {
        let mut e = entity("ame:session", "session");
        e.truths.insert("status".into(), "IN_PROGRESS".into());
        e.truths.insert("next".into(), "ship 0.2.1".into());
        e.observations
            .push("completed 2026-06-23: fixed compact".into());

        let md = render_entity_markdown(&e, "ame-session", &[]);

        assert!(md.contains("## Truths"), "truths section missing:\n{md}");
        // BTreeMap key order: next before status.
        assert!(md.contains("* next: ship 0.2.1"));
        assert!(md.contains("* status: IN_PROGRESS"));
        assert!(md.contains("## Observations"));
        assert!(md.contains("* completed 2026-06-23: fixed compact"));
    }

    #[test]
    fn test_render_truths_only_omits_empty_sections() {
        let mut e = entity("ame:task-1", "task");
        e.truths.insert("status".into(), "DONE".into());

        let md = render_entity_markdown(&e, "ame-task-1", &[]);

        assert!(md.contains("## Truths"));
        assert!(!md.contains("## Observations"));
        assert!(!md.contains("## Relations"));
    }

    #[test]
    fn test_render_relations_both_directions() {
        let e = entity("ame:task-1", "task");
        let rels = vec![
            crate::model::RelationInput {
                from: "ame:task-1".into(),
                to: "ame:epic".into(),
                relation_type: "part_of".into(),
            },
            crate::model::RelationInput {
                from: "ame:task-2".into(),
                to: "ame:task-1".into(),
                relation_type: "depends_on".into(),
            },
        ];

        let md = render_entity_markdown(&e, "ame-task-1", &rels);

        assert!(md.contains("* part_of [[ame-epic]]"));
        assert!(md.contains("* [[ame-task-2]] is depends_on of this"));
    }
}
