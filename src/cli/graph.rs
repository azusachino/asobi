use super::commands::Commands;
use super::dispatch::import_graph;
use super::output::*;
use crate::api::{
    BackupStore, GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore, Stats,
};
use anyhow::Result;
use tracing::info;

pub(crate) fn run(backend: &crate::storage::Storage, command: Commands, json: bool) -> Result<()> {
    match command {
        Commands::New {
            pairs,
            observations,
        } => {
            if pairs.is_empty() || pairs.len() % 2 != 0 {
                anyhow::bail!(
                    "new expects one or more `NAME TYPE` pairs (got {} arguments)",
                    pairs.len()
                );
            }
            let entities: Vec<crate::model::EntityInput> = pairs
                .chunks_exact(2)
                .map(|c| crate::model::EntityInput {
                    name: c[0].clone(),
                    entity_type: c[1].clone(),
                    observations: observations.clone(),
                })
                .collect();
            let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
            backend.create_entities(entities)?;
            info!("{} entit{} created.", names.len(), plural(names.len()));
            if json {
                emit_nodes(backend, names)?;
            }
        }
        Commands::Link { triples } => {
            if triples.is_empty() || triples.len() % 3 != 0 {
                anyhow::bail!(
                    "link expects one or more `FROM TO TYPE` triples (got {} arguments)",
                    triples.len()
                );
            }
            let relations: Vec<crate::model::RelationInput> = triples
                .chunks_exact(3)
                .map(|c| crate::model::RelationInput {
                    from: c[0].clone(),
                    to: c[1].clone(),
                    relation_type: c[2].clone(),
                })
                .collect();
            let involved: Vec<String> = relations
                .iter()
                .flat_map(|r| [r.from.clone(), r.to.clone()])
                .collect();
            let count = relations.len();
            backend.create_relations(relations)?;
            info!("{} relation{} created.", count, suffix(count));
            if json {
                emit_nodes(backend, involved)?;
            }
        }
        Commands::Obs { name, contents } => {
            let paths = crate::paths::AsobiPaths::resolve();
            let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(paths.observation_limit.unwrap_or(200));
            backend.add_observations(
                vec![crate::model::ObservationInput {
                    entity_name: name.clone(),
                    contents,
                }],
                limit,
            )?;
            info!("Observation added.");
            if json {
                emit_nodes(backend, vec![name])?;
            }
        }
        Commands::Truth { name, key, value } => {
            backend.truth_upsert(&name, &key, &value)?;
            info!("Truth added.");
            if json {
                emit_nodes(backend, vec![name])?;
            }
        }
        Commands::RmTruth { name, key } => {
            backend.truth_delete(&name, &key)?;
            info!("Truth deleted.");
            if json {
                emit_nodes(backend, vec![name])?;
            }
        }
        Commands::History { name, key } => {
            let history = backend.truth_history(&name, key.as_deref())?;
            print_json(history)?;
        }
        Commands::Rm { names } => {
            let deleted = names.clone();
            backend.delete_entities(names)?;
            info!("Entities deleted.");
            if json {
                print_json(DeletedReceipt { deleted })?;
            }
        }
        Commands::RmObs { name, content, id } => {
            if id {
                let parsed_id = content.parse::<i64>().map_err(|_| {
                    anyhow::anyhow!(
                        "Invalid observation ID: '{}'. Expected an integer.",
                        content
                    )
                })?;
                backend.delete_observation_by_id(&name, parsed_id)?;
            } else {
                backend.delete_observations(vec![crate::model::ObservationDeletion {
                    entity_name: name.clone(),
                    observations: vec![content],
                }])?;
            }
            info!("Observations deleted.");
            if json {
                emit_nodes(backend, vec![name])?;
            }
        }
        Commands::UpdateObs {
            name,
            old_content,
            new_content,
            id,
        } => {
            if id {
                let parsed_id = old_content.parse::<i64>().map_err(|_| {
                    anyhow::anyhow!(
                        "Invalid observation ID: '{}'. Expected an integer.",
                        old_content
                    )
                })?;
                backend.update_observation_by_id(&name, parsed_id, &new_content)?;
            } else {
                backend.update_observation(&name, &old_content, &new_content)?;
            }
            info!("Observation updated.");
            if json {
                emit_nodes(backend, vec![name])?;
            }
        }
        Commands::Unlink {
            from,
            to,
            relation_type,
        } => {
            backend.delete_relations(vec![crate::model::RelationInput {
                from: from.clone(),
                to: to.clone(),
                relation_type,
            }])?;
            info!("Relations deleted.");
            if json {
                emit_nodes(backend, vec![from, to])?;
            }
        }
        Commands::Graph => {
            let graph = backend.read_graph()?;
            print_json(graph)?;
        }
        Commands::Search {
            query,
            limit,
            filters,
        } => {
            let mut parsed_filters = Vec::new();
            for f in &filters {
                if let Some((k, v)) = f.split_once('=') {
                    parsed_filters.push((k.trim().to_string(), v.trim().to_string()));
                } else {
                    anyhow::bail!("Invalid filter format: '{}'. Expected KEY=VALUE.", f);
                }
            }
            let query_str = query.unwrap_or_default();
            let graph = backend.search_nodes(SearchQuery {
                query: query_str,
                limit,
                filters: parsed_filters,
            })?;
            print_json(graph)?;
        }
        Commands::Show {
            names,
            expand,
            with_ids,
        } => {
            let graph = backend.open_nodes(OpenNodes {
                names,
                with_ids,
                expand,
            })?;
            print_json(graph)?;
        }
        Commands::Stats { per_entity } => {
            let location = backend.location()?;

            let Stats {
                entities,
                relations,
                observations,
            } = backend.stats()?;
            if json {
                let entities_detailed = if per_entity {
                    let paths = crate::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity()?;
                    Some(
                        list.iter()
                            .map(|(name, count)| {
                                let pct = if limit > 0 {
                                    (*count as f64 / limit as f64) * 100.0
                                } else {
                                    0.0
                                };
                                EntityStatsDetail {
                                    name: name.clone(),
                                    observation_count: *count,
                                    limit,
                                    percentage: pct,
                                    critical: limit > 0 && *count >= (limit * 80 / 100),
                                }
                            })
                            .collect(),
                    )
                } else {
                    None
                };

                print_json(StatsReceipt {
                    entities,
                    relations,
                    observations,
                    database_path: location.database_path,
                    journal_mode: location.journal_mode,
                    schema_version: location.schema_version,
                    entities_detailed,
                })?;
            } else {
                println!("Database Path:  {}", location.database_path);
                println!("Journal Mode:   {}", location.journal_mode);
                println!("Schema Version: {}", location.schema_version);
                println!("Knowledge Graph Statistics:");
                println!("  Entities:     {}", entities);
                println!("  Relations:    {}", relations);
                println!("  Observations: {}", observations);

                if per_entity {
                    let paths = crate::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity()?;
                    if !list.is_empty() {
                        println!("\nEntities by Observation Count:");
                        for (name, count) in &list {
                            let pct = if limit > 0 {
                                (*count as f64 / limit as f64) * 100.0
                            } else {
                                0.0
                            };
                            if limit > 0 && *count >= (limit * 80 / 100) {
                                println!(
                                    "  {:_<40} {} / {} (CRITICAL: {:.1}%)",
                                    name, count, limit, pct
                                );
                            } else {
                                println!("  {:_<40} {}", name, count);
                            }
                        }
                    }
                }
            }
        }
        Commands::Capabilities => {
            let capabilities = backend.capabilities()?;
            let health = backend.health()?;
            print_json(CapabilitiesReceipt {
                api_version: crate::api::API_VERSION,
                capabilities,
                health,
            })?;
        }

        Commands::Export {
            output,
            scope,
            rationale,
        } => {
            let graph = if scope.is_empty() {
                backend.read_graph_full()?
            } else {
                backend.read_graph_scoped(&scope, rationale)?
            };
            if let Some(path) = output {
                let json = serde_json::to_string_pretty(&graph)?;
                std::fs::write(&path, json)?;
                crate::application::restrict_permissions(std::path::Path::new(&path), 0o600)?;
                info!("Graph exported to {}", path);
            } else {
                print_json(graph)?;
            }
        }
        Commands::Import { file } => {
            let content = std::fs::read_to_string(&file)?;
            let graph: crate::model::Graph = serde_json::from_str(&content)?;

            let had_entities = !graph.entities.is_empty();
            let had_relations = !graph.relations.is_empty();
            import_graph(backend, graph)?;
            if had_entities {
                info!("Imported entities, observations, and truths.");
            }
            if had_relations {
                info!("Imported relations.");
            }
            info!("Import complete.");
        }
        Commands::Reset { force } => {
            if !force {
                use std::io::Write;
                print!("Are you sure you want to completely clear the knowledge graph? [y/N]: ");
                std::io::stdout().flush()?;
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    info!("Reset aborted.");
                    return Ok(());
                }
            }
            backend.reset()?;
            info!("Knowledge graph reset successfully.");
        }
        Commands::Backup { output, keep } => {
            let receipt = backend.backup(crate::api::BackupRequest {
                destination: output.map(std::path::PathBuf::from).unwrap_or_default(),
                keep,
            })?;
            info!("Backup written to {}", receipt.path.display());
        }
        Commands::Restore { .. } => unreachable!("restore handled before borrowing storage"),
        _ => unreachable!("non-graph command routed to graph handler"),
    }
    Ok(())
}

fn suffix(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "y" } else { "ies" }
}
