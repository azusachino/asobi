use crate::api::{GraphStore, OpenNodes};
use anyhow::Result;
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeletedReceipt {
    pub(crate) deleted: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntityStatsDetail {
    pub(crate) name: String,
    pub(crate) observation_count: usize,
    pub(crate) limit: usize,
    pub(crate) percentage: f64,
    pub(crate) critical: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatsReceipt {
    pub(crate) entities: usize,
    pub(crate) relations: usize,
    pub(crate) observations: usize,
    pub(crate) database_path: String,
    pub(crate) journal_mode: String,
    pub(crate) schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entities_detailed: Option<Vec<EntityStatsDetail>>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilitiesReceipt {
    pub(crate) api_version: u32,
    pub(crate) capabilities: crate::api::BackendCapabilities,
    pub(crate) health: crate::api::BackendHealth,
}

/// Print the named entities (and the relations among them) as pretty JSON to
/// stdout — the `--json` echo after a mutation, so a caller can confirm the
/// write without a second `show` round-trip. Names are normalized inside
/// `open_nodes`, so raw user input matches what was just stored.
pub(crate) fn emit_nodes(store: &impl GraphStore, names: Vec<String>) -> Result<()> {
    let graph = store.open_nodes(OpenNodes {
        names,
        ..Default::default()
    })?;
    print_json(graph)?;
    Ok(())
}

pub(crate) fn print_json<T: Serialize>(value: T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

const CLI_SCHEMA_VERSION: u32 = 1;

fn schema_for_data<T: JsonSchema>() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(T))
        .expect("response schemas must serialize to JSON");
    schema["x-asobi-schema-version"] = serde_json::json!(CLI_SCHEMA_VERSION);
    schema
}

/// A command name paired with a builder for its payload's JSON Schema.
type SchemaRow = (&'static str, fn() -> serde_json::Value);

/// The one source of truth mapping a command to the JSON Schema of its
/// machine-readable payload. Most commands echo the affected `Graph`; the rest
/// have their own receipt type. Adding a command means adding one row here.
fn schema_registry() -> Vec<SchemaRow> {
    use crate::model::Graph;
    let rows: Vec<SchemaRow> = vec![
        ("capabilities", schema_for_data::<CapabilitiesReceipt>),
        ("export", schema_for_data::<Graph>),
        ("graph", schema_for_data::<Graph>),
        ("history", schema_for_data::<Vec<crate::api::TruthVersion>>),
        ("link", schema_for_data::<Graph>),
        ("new", schema_for_data::<Graph>),
        ("obs", schema_for_data::<Graph>),
        ("rm", schema_for_data::<DeletedReceipt>),
        ("rm-obs", schema_for_data::<Graph>),
        ("rm-truth", schema_for_data::<Graph>),
        ("search", schema_for_data::<Graph>),
        ("show", schema_for_data::<Graph>),
        ("stats", schema_for_data::<StatsReceipt>),
        ("tasks-close", schema_for_data::<crate::tasks::TaskReceipt>),
        (
            "tasks-dispatch",
            schema_for_data::<crate::tasks::TaskReceipt>,
        ),
        ("tasks-list", schema_for_data::<crate::model::Graph>),
        ("tasks-plan", schema_for_data::<crate::model::Graph>),
        ("tasks-sync", schema_for_data::<crate::tasks::TaskReceipt>),
        ("truth", schema_for_data::<Graph>),
        ("unlink", schema_for_data::<Graph>),
        ("update-obs", schema_for_data::<Graph>),
    ];
    rows
}

pub(crate) fn emit_schema(command: Option<&str>) -> Result<()> {
    let registry = schema_registry();
    if let Some(command) = command {
        let (_, schema_of) = registry
            .iter()
            .find(|(name, _)| *name == command)
            .ok_or_else(|| anyhow::anyhow!("unknown schema command: {command}"))?;
        print_json(schema_of())
    } else {
        let commands: std::collections::BTreeMap<_, _> = registry
            .iter()
            .map(|(name, schema_of)| (*name, schema_of()))
            .collect();
        print_json(serde_json::json!({
            "schemaVersion": CLI_SCHEMA_VERSION,
            "commands": commands,
        }))
    }
}
