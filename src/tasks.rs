use crate::api::{GraphStore, SearchQuery, SearchStore};
use anyhow::Result;
use clap::Subcommand;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskReceipt {
    pub action: &'static str,
    pub entity: String,
    pub status: String,
}

const TASK_STATUSES: &[&str] = &[
    "READY_TO_DISPATCH",
    "DISPATCHED",
    "REVIEW",
    "AWAITING_VERIFY",
    "DONE",
];

#[derive(Subcommand, Debug)]
pub enum TasksCommands {
    /// Create an epic and its dispatchable child tasks
    Plan {
        /// Epic entity name
        epic: String,
        /// Objective stored on the epic
        #[arg(long)]
        objective: String,
        /// Child task title (repeat for each task, in execution order)
        #[arg(long = "task", value_name = "TITLE", required = true)]
        tasks: Vec<String>,
    },
    /// Show an epic task board, or all task entities when no epic is given
    List { epic: Option<String> },
    /// Mark the next ready task, or the named task, as dispatched
    Dispatch {
        task: Option<String>,
        #[arg(long, default_value = "lead")]
        agent: String,
    },
    /// Record implementation/review notes and advance a task status
    Sync {
        task: String,
        #[arg(long = "note")]
        notes: Vec<String>,
        #[arg(long, default_value = "REVIEW")]
        status: String,
    },
    /// Mark an all-DONE epic as closed and promote optional lessons
    Close {
        epic: String,
        #[arg(long = "lesson")]
        lessons: Vec<String>,
    },
}

pub fn run(
    backend: &(impl GraphStore + SearchStore),
    subcommand: Option<TasksCommands>,
    json: bool,
) -> Result<()> {
    match subcommand {
        None => {
            println!("Use `asobi tasks --help` to see task-dispatcher commands.");
        }
        Some(TasksCommands::Plan {
            epic,
            objective,
            tasks,
        }) => {
            if epic.trim().is_empty() || objective.trim().is_empty() {
                anyhow::bail!("plan requires a non-empty epic and objective");
            }
            if tasks.iter().any(|title| title.trim().is_empty()) {
                anyhow::bail!("plan task titles must be non-empty");
            }
            let epic_name = epic.clone();
            let child_names: Vec<String> = tasks
                .iter()
                .enumerate()
                .map(|(idx, _)| format!("{epic}:task-{}", idx + 1))
                .collect();
            let existing = backend.open_nodes(crate::api::OpenNodes {
                names: std::iter::once(epic.clone())
                    .chain(child_names.iter().cloned())
                    .collect(),
                ..Default::default()
            })?;
            if !existing.entities.is_empty() {
                anyhow::bail!("plan target already exists: {}", existing.entities[0].name);
            }
            backend.create_entities(vec![crate::model::EntityInput {
                name: epic.clone(),
                entity_type: "task".to_string(),
                observations: vec![format!("scope: {}", objective)],
            }])?;

            backend.create_entities(
                child_names
                    .iter()
                    .zip(&tasks)
                    .map(|(name, title)| crate::model::EntityInput {
                        name: name.clone(),
                        entity_type: "task".to_string(),
                        observations: vec![format!("plan: {title}")],
                    })
                    .collect(),
            )?;
            backend.truth_upsert(&epic, "objective", &objective)?;
            for (name, title) in child_names.iter().zip(&tasks) {
                backend.truth_upsert(name, "title", title)?;
                backend.truth_upsert(name, "status", "READY_TO_DISPATCH")?;
            }
            backend.create_relations(
                child_names
                    .iter()
                    .map(|name| crate::model::RelationInput {
                        from: name.clone(),
                        to: epic.clone(),
                        relation_type: "part_of".to_string(),
                    })
                    .collect(),
            )?;
            if json {
                let graph = backend.open_nodes(crate::api::OpenNodes {
                    names: vec![epic_name],
                    expand: vec!["part_of".to_string()],
                    ..Default::default()
                })?;
                print_json(graph)?;
            } else {
                println!("Planned {} with {} task(s).", epic, tasks.len());
            }
        }
        Some(TasksCommands::List { epic }) => {
            let graph = if let Some(epic) = epic {
                let graph = backend.open_nodes(crate::api::OpenNodes {
                    names: vec![epic.clone()],
                    expand: vec!["part_of".to_string()],
                    ..Default::default()
                })?;
                if graph.entities.is_empty() {
                    anyhow::bail!("epic not found: {epic}");
                }
                graph
            } else {
                let mut graph = backend.read_graph()?;
                let task_names: std::collections::HashSet<_> = graph
                    .entities
                    .iter()
                    .filter(|entity| entity.entity_type == "task")
                    .map(|entity| entity.name.clone())
                    .collect();
                graph
                    .entities
                    .retain(|entity| task_names.contains(&entity.name));
                graph.relations.retain(|relation| {
                    task_names.contains(&relation.from) && task_names.contains(&relation.to)
                });
                graph
            };
            print_json(graph)?;
        }
        Some(TasksCommands::Dispatch { task, agent }) => {
            if agent.trim().is_empty() {
                anyhow::bail!("dispatch agent must be non-empty");
            }
            let task = if let Some(task) = task {
                task
            } else {
                let graph = backend.search_nodes(SearchQuery {
                    query: String::new(),
                    limit: 100,
                    filters: vec![("status".to_string(), "READY_TO_DISPATCH".to_string())],
                })?;
                graph
                    .entities
                    .into_iter()
                    .find(|entity| entity.entity_type == "task")
                    .map(|entity| entity.name)
                    .ok_or_else(|| anyhow::anyhow!("no READY_TO_DISPATCH task found"))?
            };
            let graph = backend.open_nodes(crate::api::OpenNodes {
                names: vec![task.clone()],
                ..Default::default()
            })?;
            let entity = graph
                .entities
                .first()
                .ok_or_else(|| anyhow::anyhow!("task not found: {task}"))?;
            if entity.entity_type != "task" {
                anyhow::bail!("entity is not a task: {task}");
            }
            if entity.truths.get("status").map(String::as_str) != Some("READY_TO_DISPATCH") {
                anyhow::bail!("task is not READY_TO_DISPATCH: {task}");
            }
            backend.truth_upsert(&task, "status", "DISPATCHED")?;
            backend.add_observations(
                vec![crate::model::ObservationInput {
                    entity_name: task.clone(),
                    contents: vec![format!("dispatched to {agent}")],
                }],
                observation_limit(),
            )?;
            if json {
                print_json(TaskReceipt {
                    action: "dispatch",
                    entity: task,
                    status: "DISPATCHED".to_string(),
                })?;
            } else {
                println!("Dispatched {task} to {agent}.");
            }
        }
        Some(TasksCommands::Sync {
            task,
            notes,
            status,
        }) => {
            let graph = backend.open_nodes(crate::api::OpenNodes {
                names: vec![task.clone()],
                ..Default::default()
            })?;
            let entity = graph
                .entities
                .first()
                .ok_or_else(|| anyhow::anyhow!("task not found: {task}"))?;
            if entity.entity_type != "task" {
                anyhow::bail!("entity is not a task: {task}");
            }
            if !TASK_STATUSES.contains(&status.as_str()) && !status.starts_with("BLOCKED_ON ") {
                anyhow::bail!("invalid task status: {status}");
            }
            if graph.entities.is_empty() {
                anyhow::bail!("task not found: {task}");
            }
            if !notes.is_empty() {
                backend.add_observations(
                    vec![crate::model::ObservationInput {
                        entity_name: task.clone(),
                        contents: notes,
                    }],
                    observation_limit(),
                )?;
            }
            backend.truth_upsert(&task, "status", &status)?;
            if json {
                print_json(TaskReceipt {
                    action: "sync",
                    entity: task,
                    status,
                })?;
            } else {
                println!("Synced {task} as {status}.");
            }
        }
        Some(TasksCommands::Close { epic, lessons }) => {
            let graph = backend.open_nodes(crate::api::OpenNodes {
                names: vec![epic.clone()],
                expand: vec!["part_of".to_string()],
                ..Default::default()
            })?;
            let children: Vec<_> = graph
                .entities
                .iter()
                .filter(|entity| entity.name != epic && entity.entity_type == "task")
                .collect();
            if graph.entities.is_empty() {
                anyhow::bail!("epic not found: {epic}");
            }
            if children.is_empty() {
                anyhow::bail!("cannot close {epic}: no child tasks found");
            }
            if children
                .iter()
                .any(|entity| entity.truths.get("status").map(String::as_str) != Some("DONE"))
            {
                anyhow::bail!("cannot close {epic}: every child task must be DONE");
            }
            let project = epic.split(':').next().unwrap_or(&epic).to_string();
            if !lessons.is_empty() && !graph.entities.iter().any(|entity| entity.name == project) {
                backend.create_entities(vec![crate::model::EntityInput {
                    name: project.clone(),
                    entity_type: "project".to_string(),
                    observations: vec![],
                }])?;
            }
            if !lessons.is_empty() {
                backend.add_observations(
                    vec![crate::model::ObservationInput {
                        entity_name: project,
                        contents: lessons,
                    }],
                    observation_limit(),
                )?;
            }
            backend.truth_upsert(&epic, "status", "DONE")?;
            backend.add_observations(
                vec![crate::model::ObservationInput {
                    entity_name: epic.clone(),
                    contents: vec![format!(
                        "outcome: closed {}",
                        chrono::Local::now().format("%Y-%m-%d")
                    )],
                }],
                observation_limit(),
            )?;
            if json {
                print_json(TaskReceipt {
                    action: "close",
                    entity: epic,
                    status: "DONE".to_string(),
                })?;
            } else {
                println!("Closed {epic}.");
            }
        }
    };
    Ok(())
}

fn observation_limit() -> usize {
    let paths = crate::paths::AsobiPaths::resolve();
    std::env::var("ASOBI_OBSERVATION_LIMIT")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(paths.observation_limit.unwrap_or(200))
}

fn print_json<T: Serialize>(value: T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
