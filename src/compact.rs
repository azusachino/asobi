use crate::api::v1::{DocumentStore, GraphStore};
use crate::ingest::ingest_file;
use crate::model::EntityOutput;
use crate::normalize::slugify;
use anyhow::Result;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub async fn sync_graph_to_markdown(
    graph_store: &impl GraphStore,
    document_store: &impl DocumentStore,
    embedder: &impl crate::embed::EmbeddingProvider,
) -> Result<usize> {
    let paths = crate::paths::AsobiPaths::resolve();
    let topics_dir = paths.topics_dir;
    if !topics_dir.exists() {
        std::fs::create_dir_all(&topics_dir)?;
    }

    let graph = graph_store.read_graph_full().await?;
    let today = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let mut count = 0;

    for entity in &graph.entities {
        if !should_sync(&entity.entity_type) {
            continue;
        }

        let slug = slugify(&entity.name);
        let file_path = topics_dir.join(format!("{}.md", slug));

        let mut compacted_time = today.clone();
        let mut should_write = true;

        if file_path.exists() {
            if let Ok(existing) = std::fs::read_to_string(&file_path) {
                if let Some(old_time) = crate::frontmatter::parse(&existing)
                    .and_then(|fm| fm.get("compacted").map(|s| s.to_string()))
                {
                    let content_with_old_time =
                        render_entity_markdown(entity, &slug, &graph.relations, &old_time);
                    if existing == content_with_old_time {
                        should_write = false;
                        compacted_time = old_time;
                    }
                }
            }
        }

        if should_write {
            let content = render_entity_markdown(entity, &slug, &graph.relations, &compacted_time);
            std::fs::File::create(&file_path)?.write_all(content.as_bytes())?;
            // Refresh the Vector/FTS tier from the rendered file.
            ingest_file(&file_path, document_store, embedder).await?;
            count += 1;
        }
    }

    Ok(count)
}

/// The recall tier (Markdown topics + FTS/vector index) holds durable
/// *knowledge*, not volatile *state* or self-indexing content. We skip:
///
/// - `session` / `task` (epics are `task` too, so they skip with their tasks):
///   operational state that flips constantly and is already cheaply queryable
///   from the graph via `search --where status=…` / `show`. Embedding it only
///   churns the index and pollutes semantic `query` results; full archival
///   lives in `export` / `backup`, not here.
/// - `skill`: the installer already chunks the full skill body into the
///   document tier under `topic_id = entity_name`. Syncing it here would emit a
///   second topic under the slug and double-index the same content, so a skill
///   stays graph- and installer-owned — recall it via `query` or `skills show`.
///
/// Denylist (not allowlist) so new knowledge types persist by default.
fn should_sync(entity_type: &str) -> bool {
    !matches!(entity_type, "session" | "task" | "skill")
}

/// Write the YAML frontmatter block. Beyond the `title`/`type`/`slug` identity
/// keys it promotes machine-readable metadata so strict consumers (Obsidian,
/// Dataview) can query topics without reading the body:
/// - `aliases`: the raw, un-slugified entity name so wikilinks resolve to it.
/// - `observations` / `relations`: trail + edge counts for sorting/filtering.
/// - `compacted`: the date this topic was last written.
/// - `truth_<key>`: each truth as a property. Prefixed so a truth can never
///   collide with a reserved identity key (`title`/`type`/`slug`/…).
/// - one key per outgoing relation type, value a wikilink (or a list when the
///   type repeats).
///
/// Every value is routed through [`crate::frontmatter::quote`] (counts stay
/// bare integers) so it round-trips through [`crate::frontmatter::parse`].
fn render_frontmatter(
    out: &mut String,
    entity: &EntityOutput,
    slug: &str,
    relations: &[crate::model::RelationInput],
    compacted: &str,
) {
    use crate::frontmatter::quote;

    // Outgoing edges grouped by type; BTreeMap keeps the output deterministic.
    let mut outgoing: std::collections::BTreeMap<&str, Vec<String>> =
        std::collections::BTreeMap::new();
    for rel in relations.iter().filter(|r| r.from == entity.name) {
        outgoing
            .entry(rel.relation_type.as_str())
            .or_default()
            .push(format!("[[{}]]", slugify(&rel.to)));
    }
    let relation_count: usize = outgoing.values().map(Vec::len).sum();

    let _ = writeln!(out, "---");
    let _ = writeln!(out, "title: {}", quote(&entity.name));
    let _ = writeln!(out, "type: {}", quote(&entity.entity_type));
    let _ = writeln!(out, "slug: {}", quote(slug));
    let _ = writeln!(out, "aliases: {}", quote(&entity.name));
    let _ = writeln!(out, "observations: {}", entity.observations.len());
    let _ = writeln!(out, "relations: {}", relation_count);
    let _ = writeln!(out, "compacted: {}", quote(compacted));

    // BTreeMap iterates in key order, so truth properties are deterministic.
    for (key, value) in &entity.truths {
        let _ = writeln!(out, "truth_{}: {}", key, quote(value));
    }

    for (rtype, targets) in &outgoing {
        if let [single] = targets.as_slice() {
            let _ = writeln!(out, "{}: {}", rtype, quote(single));
        } else {
            let list = targets
                .iter()
                .map(|t| quote(t))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(out, "{}: [{}]", rtype, list);
        }
    }

    let _ = writeln!(out, "---\n");
}

/// Render one entity to its Markdown topic: frontmatter plus Truths
/// (current state), Observations (trail), and Relations sections.
fn render_entity_markdown(
    entity: &EntityOutput,
    slug: &str,
    relations: &[crate::model::RelationInput],
    compacted: &str,
) -> String {
    let mut out = String::new();

    render_frontmatter(&mut out, entity, slug, relations, compacted);

    let _ = writeln!(out, "# {}\n", entity.name);

    if !entity.truths.is_empty() {
        let _ = writeln!(out, "## Truths\n");
        let _ = writeln!(out, "| Property | Value |");
        let _ = writeln!(out, "| :--- | :--- |");
        // BTreeMap iterates in key order, so output is deterministic.
        for (key, value) in &entity.truths {
            let _ = writeln!(out, "| **{}** | {} |", key, value);
        }
        out.push('\n');
    }

    if !entity.observations.is_empty() {
        let _ = writeln!(out, "## Observations\n");
        let mut unique_obs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for obs in &entity.observations {
            if seen.insert(obs) {
                unique_obs.push(obs);
            }
        }
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
        let _ = writeln!(out, "| Direction | Relation | Entity |");
        let _ = writeln!(out, "| :--- | :--- | :--- |");
        for rel in related {
            if rel.from == entity.name {
                let _ = writeln!(
                    out,
                    "| Outgoing | `{}` | [[{}]] |",
                    rel.relation_type,
                    slugify(&rel.to)
                );
            } else {
                let _ = writeln!(
                    out,
                    "| Incoming | `{}` | [[{}]] |",
                    rel.relation_type,
                    slugify(&rel.from)
                );
            }
        }
        out.push('\n');
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
    _store: &impl crate::api::v1::DocumentMaintenanceStore,
    _threshold: f32,
) -> anyhow::Result<Vec<Vec<String>>> {
    _store
        .find_duplicate_clusters(_threshold)
        .await
        .map_err(Into::into)
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
        // Volatile operational state stays graph-only.
        assert!(!should_sync("session"));
        assert!(!should_sync("task"));
        // Skills are already indexed by the installer; syncing would duplicate.
        assert!(!should_sync("skill"));
    }

    struct FakeMaintenance;

    impl crate::api::v1::DocumentMaintenanceStore for FakeMaintenance {
        async fn find_duplicate_clusters(
            &self,
            _threshold: f32,
        ) -> crate::api::v1::ApiResult<Vec<Vec<String>>> {
            Ok(vec![vec!["topic-a".into(), "topic-b".into()]])
        }
    }

    #[tokio::test]
    async fn test_find_duplicate_clusters_uses_storage_capability() {
        let clusters = find_duplicate_clusters(&FakeMaintenance, 0.99)
            .await
            .unwrap();
        assert_eq!(clusters, vec![vec!["topic-a", "topic-b"]]);
    }

    fn entity(name: &str, entity_type: &str) -> EntityOutput {
        EntityOutput {
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            observations: Vec::new(),
            truths: std::collections::BTreeMap::new(),
            observation_count: 0,
            body: None,
            observations_detailed: None,
        }
    }

    #[test]
    fn test_render_includes_truths_and_observations() {
        let mut e = entity("ame:session", "session");
        e.truths.insert("status".into(), "IN_PROGRESS".into());
        e.truths.insert("next".into(), "ship 0.2.1".into());
        e.observations
            .push("completed 2026-06-23: fixed compact".into());

        let md = render_entity_markdown(&e, "ame-session", &[], "2026-06-23");

        assert!(md.contains("## Truths"), "truths section missing:\n{md}");
        // BTreeMap key order: next before status.
        assert!(md.contains("| **next** | ship 0.2.1 |"));
        assert!(md.contains("| **status** | IN_PROGRESS |"));
        assert!(md.contains("## Observations"));
        assert!(md.contains("* completed 2026-06-23: fixed compact"));
    }

    #[test]
    fn test_render_quotes_frontmatter_values() {
        // A decision concept actually gets synced (should_sync is true) and its
        // `:`-separated name is the real strict-YAML hazard quoting guards.
        let e = entity("asobi:decision:no-pwa", "concept");
        let md = render_entity_markdown(&e, "asobi-decision-no-pwa", &[], "2026-06-23");

        assert!(
            md.contains("title: \"asobi:decision:no-pwa\""),
            "title not YAML-quoted:\n{md}"
        );
        assert!(md.contains("type: \"concept\""));
        assert!(md.contains("slug: \"asobi-decision-no-pwa\""));
    }

    #[test]
    fn test_frontmatter_promotes_truths_counts_and_relations() {
        let mut e = entity("asobi:decision:no-pwa", "concept");
        e.truths.insert("status".into(), "ACCEPTED".into());
        e.observations.push("decision: ship native".into());
        let rels = vec![crate::model::RelationInput {
            from: "asobi:decision:no-pwa".into(),
            to: "asobi".into(),
            relation_type: "part_of".into(),
        }];

        let md = render_entity_markdown(&e, "asobi-decision-no-pwa", &rels, "2026-06-23");
        let fm = crate::frontmatter::parse(&md).expect("frontmatter parses");

        // Truths promoted as prefixed properties; counts + aliases + date present.
        assert_eq!(fm.get("truth_status"), Some("ACCEPTED"));
        assert_eq!(fm.get("aliases"), Some("asobi:decision:no-pwa"));
        assert_eq!(fm.get("observations"), Some("1"));
        assert_eq!(fm.get("relations"), Some("1"));
        assert_eq!(fm.get("compacted"), Some("2026-06-23"));
        // Outgoing relation as a wikilink property.
        assert_eq!(fm.get("part_of"), Some("[[asobi]]"));
    }

    #[test]
    fn test_frontmatter_repeated_relation_type_is_a_list() {
        let e = entity("ame:task-3", "task");
        let rels = vec![
            crate::model::RelationInput {
                from: "ame:task-3".into(),
                to: "ame:task-1".into(),
                relation_type: "depends_on".into(),
            },
            crate::model::RelationInput {
                from: "ame:task-3".into(),
                to: "ame:task-2".into(),
                relation_type: "depends_on".into(),
            },
        ];

        let md = render_entity_markdown(&e, "ame-task-3", &rels, "2026-06-23");
        assert!(
            md.contains("depends_on: [\"[[ame-task-1]]\", \"[[ame-task-2]]\"]"),
            "repeated relation type not a list:\n{md}"
        );
    }

    #[test]
    fn test_render_truths_only_omits_empty_sections() {
        let mut e = entity("ame:task-1", "task");
        e.truths.insert("status".into(), "DONE".into());

        let md = render_entity_markdown(&e, "ame-task-1", &[], "2026-06-23");

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

        let md = render_entity_markdown(&e, "ame-task-1", &rels, "2026-06-23");

        assert!(md.contains("| Outgoing | `part_of` | [[ame-epic]] |"));
        assert!(md.contains("| Incoming | `depends_on` | [[ame-task-2]] |"));
    }
}
