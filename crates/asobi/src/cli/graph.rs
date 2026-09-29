use super::commands::Commands;
use super::output::*;
use anyhow::Result;
use asobi_core::api::{GraphStore, MaintenanceStore, OpenNodes, SearchQuery, SearchStore, Stats};
use tracing::info;

pub(crate) async fn run(
    backend: &crate::storage::Storage,
    command: Commands,
    json: bool,
) -> Result<()> {
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
            let entities: Vec<asobi_core::model::EntityInput> = pairs
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| asobi_core::model::EntityInput {
                    name: c[0].clone(),
                    entity_type: c[1].clone(),
                    observations: observations.clone(),
                })
                .collect();
            let names: Vec<String> = entities.iter().map(|e| e.name.clone()).collect();
            backend.create_entities(entities).await?;
            info!("{} entit{} created.", names.len(), plural(names.len()));
            if json {
                emit_nodes(backend, names).await?;
            }
        }
        Commands::Link { triples } => {
            if triples.is_empty() || triples.len() % 3 != 0 {
                anyhow::bail!(
                    "link expects one or more `FROM TO TYPE` triples (got {} arguments)",
                    triples.len()
                );
            }
            let relations: Vec<asobi_core::model::RelationInput> = triples
                .as_chunks::<3>()
                .0
                .iter()
                .map(|c| asobi_core::model::RelationInput {
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
            backend.create_relations(relations).await?;
            info!("{} relation{} created.", count, suffix(count));
            if json {
                emit_nodes(backend, involved).await?;
            }
        }
        Commands::Obs { name, contents } => {
            let paths = asobi_core::paths::AsobiPaths::resolve();
            let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(paths.observation_limit.unwrap_or(200));
            backend
                .add_observations(
                    vec![asobi_core::model::ObservationInput {
                        entity_name: name.clone(),
                        contents,
                    }],
                    limit,
                )
                .await?;
            info!("Observation added.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::Truth { name, key, value } => {
            backend.truth_upsert(&name, &key, &value).await?;
            info!("Truth added.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::RmTruth { name, key } => {
            backend.truth_delete(&name, &key).await?;
            info!("Truth deleted.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::Rm { names } => {
            let deleted = names.clone();
            backend.delete_entities(names).await?;
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
                backend.delete_observation_by_id(&name, parsed_id).await?;
            } else {
                backend
                    .delete_observations(vec![asobi_core::model::ObservationDeletion {
                        entity_name: name.clone(),
                        observations: vec![content],
                    }])
                    .await?;
            }
            info!("Observations deleted.");
            if json {
                emit_nodes(backend, vec![name]).await?;
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
                backend
                    .update_observation_by_id(&name, parsed_id, &new_content)
                    .await?;
            } else {
                backend
                    .update_observation(&name, &old_content, &new_content)
                    .await?;
            }
            info!("Observation updated.");
            if json {
                emit_nodes(backend, vec![name]).await?;
            }
        }
        Commands::Unlink {
            from,
            to,
            relation_type,
        } => {
            backend
                .delete_relations(vec![asobi_core::model::RelationInput {
                    from: from.clone(),
                    to: to.clone(),
                    relation_type,
                }])
                .await?;
            info!("Relations deleted.");
            if json {
                emit_nodes(backend, vec![from, to]).await?;
            }
        }
        Commands::Graph => {
            let graph = backend.read_graph().await?;
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
            let graph = backend
                .search_nodes(SearchQuery {
                    query: query_str,
                    limit,
                    filters: parsed_filters,
                })
                .await?;
            print_json(graph)?;
        }
        Commands::Show {
            names,
            expand,
            with_ids,
            limit,
        } => {
            let graph = backend
                .open_nodes(OpenNodes {
                    observation_limit: limit,
                    names,
                    with_ids,
                    expand,
                })
                .await?;
            print_json(graph)?;
        }
        Commands::Stats { per_entity } => {
            let location = backend.location().await?;
            let remote = backend.is_remote().await;
            let config = crate::config::resolve(&asobi_core::paths::AsobiPaths::resolve());
            let graph = remote.then_some(config.graph);
            let endpoint = if remote {
                config
                    .remote
                    .map(|url| crate::config::display_endpoint(&url).to_string())
            } else {
                None
            };
            let owner = if remote { "server" } else { "client" };

            let Stats {
                entities,
                relations,
                observations,
            } = backend.stats().await?;
            if json {
                let entities_detailed = if per_entity {
                    let paths = asobi_core::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity().await?;
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
                    mode: if remote { "remote" } else { "local" },
                    path_owner: owner,
                    graph,
                    endpoint,
                    server_version: location.server_version,
                    entities_detailed,
                })?;
            } else {
                println!(
                    "Mode:           {}",
                    if remote { "remote" } else { "local" }
                );
                if let Some(graph) = graph {
                    println!("Graph:          {graph}");
                    if let Some(endpoint) = endpoint {
                        println!("Endpoint:       {endpoint}");
                    }
                    println!(
                        "Server Version: {}",
                        location.server_version.as_deref().unwrap_or("unknown")
                    );
                }
                println!("Database Path ({owner}): {}", location.database_path);
                println!("Journal Mode:   {}", location.journal_mode);
                println!("Schema Version: {}", location.schema_version);
                println!("Knowledge Graph Statistics:");
                println!("  Entities:     {}", entities);
                println!("  Relations:    {}", relations);
                println!("  Observations: {}", observations);

                if per_entity {
                    let paths = asobi_core::paths::AsobiPaths::resolve();
                    let limit = std::env::var("ASOBI_OBSERVATION_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(paths.observation_limit.unwrap_or(200));

                    let list = backend.stats_per_entity().await?;
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
            let capabilities = backend.capabilities().await?;
            let health = backend.health().await?;
            print_json(CapabilitiesReceipt {
                api_version: asobi_core::api::API_VERSION,
                capabilities,
                health,
            })?;
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
            backend.reset().await?;
            info!("Knowledge graph reset successfully.");
        }
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
