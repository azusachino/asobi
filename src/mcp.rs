use anyhow::Result;
use libsql::Connection;
use serde::{Deserialize, Serialize};

use crate::db;

// ── JSON-RPC wire types ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
}

// ── Domain types (shared with db.rs callers) ────────────────────────────────

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityInput {
    pub name: String,
    pub entity_type: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelationInput {
    pub from: String,
    pub to: String,
    pub relation_type: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ObservationInput {
    pub entity_name: String,
    pub contents: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationDeletion {
    pub entity_name: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EntityOutput {
    pub name: String,
    pub entity_type: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub entities: Vec<EntityOutput>,
    pub relations: Vec<RelationInput>,
}

// ── MCP params structs ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateEntitiesParams {
    entities: Vec<EntityInput>,
}

#[derive(Debug, Deserialize)]
struct CreateRelationsParams {
    relations: Vec<RelationInput>,
}

#[derive(Debug, Deserialize)]
struct AddObservationsParams {
    observations: Vec<ObservationInput>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteEntitiesParams {
    entity_names: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DeleteObservationsParams {
    deletions: Vec<ObservationDeletion>,
}

#[derive(Debug, Deserialize)]
struct DeleteRelationsParams {
    relations: Vec<RelationInput>,
}

#[derive(Debug, Deserialize)]
struct SearchNodesParams {
    query: String,
}

#[derive(Debug, Deserialize)]
struct OpenNodesParams {
    names: Vec<String>,
}

// ── Server ───────────────────────────────────────────────────────────────────

pub async fn run_server(conn: Connection) -> Result<()> {
    use std::io::{self, BufRead};
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            line.clear();
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(_) => {
                line.clear();
                continue;
            }
        };

        // JSON-RPC notifications have no id (or null id) — do not respond.
        let is_notification = req.id.is_null();
        let req_id = req.id.clone();

        let response = match dispatch(&conn, req).await {
            Ok(res) => res,
            Err(e) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: req_id,
                result: None,
                error: Some(serde_json::json!({
                    "code": -32603,
                    "message": e.to_string()
                })),
            },
        };

        if !is_notification {
            println!("{}", serde_json::to_string(&response)?);
        }
        line.clear();
    }
    Ok(())
}

async fn dispatch(conn: &Connection, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
    match req.method.as_str() {
        "initialize" => Ok(handle_initialize(req.id)),
        "notifications/initialized" => Ok(noop(req.id)),
        "tools/list" => Ok(handle_tools_list(req.id)),
        "tools/call" => handle_tools_call(conn, req.id, req.params.unwrap_or_default()).await,
        _ => Ok(method_not_found(req.id, &req.method)),
    }
}

// ── MCP handshake handlers ───────────────────────────────────────────────────

fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "rosemary",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        error: None,
    }
}

fn handle_tools_list(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::json!({
            "tools": [
                {
                    "name": "create_entities",
                    "description": "Create multiple new entities in the knowledge graph",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "entities": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "name": {"type": "string"},
                                        "entityType": {"type": "string"},
                                        "observations": {"type": "array", "items": {"type": "string"}}
                                    },
                                    "required": ["name", "entityType", "observations"]
                                }
                            }
                        },
                        "required": ["entities"]
                    }
                },
                {
                    "name": "create_relations",
                    "description": "Create relations between entities in the knowledge graph",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "relations": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "from": {"type": "string"},
                                        "to": {"type": "string"},
                                        "relationType": {"type": "string"}
                                    },
                                    "required": ["from", "to", "relationType"]
                                }
                            }
                        },
                        "required": ["relations"]
                    }
                },
                {
                    "name": "add_observations",
                    "description": "Add new observations to existing entities",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "observations": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "entityName": {"type": "string"},
                                        "contents": {"type": "array", "items": {"type": "string"}}
                                    },
                                    "required": ["entityName", "contents"]
                                }
                            }
                        },
                        "required": ["observations"]
                    }
                },
                {
                    "name": "delete_entities",
                    "description": "Delete entities and their associated relations",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "entityNames": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["entityNames"]
                    }
                },
                {
                    "name": "delete_observations",
                    "description": "Delete specific observations from entities",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "deletions": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "entityName": {"type": "string"},
                                        "observations": {"type": "array", "items": {"type": "string"}}
                                    },
                                    "required": ["entityName", "observations"]
                                }
                            }
                        },
                        "required": ["deletions"]
                    }
                },
                {
                    "name": "delete_relations",
                    "description": "Delete specific relations from the knowledge graph",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "relations": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "from": {"type": "string"},
                                        "to": {"type": "string"},
                                        "relationType": {"type": "string"}
                                    },
                                    "required": ["from", "to", "relationType"]
                                }
                            }
                        },
                        "required": ["relations"]
                    }
                },
                {
                    "name": "read_graph",
                    "description": "Read the entire knowledge graph",
                    "inputSchema": {"type": "object", "properties": {}}
                },
                {
                    "name": "search_nodes",
                    "description": "Search for nodes in the knowledge graph by name, type, or observation content",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {"type": "string"}
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "open_nodes",
                    "description": "Retrieve specific nodes and their relations by name",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "names": {"type": "array", "items": {"type": "string"}}
                        },
                        "required": ["names"]
                    }
                }
            ]
        })),
        error: None,
    }
}

// ── tools/call dispatcher ────────────────────────────────────────────────────

async fn handle_tools_call(
    conn: &Connection,
    id: serde_json::Value,
    params: serde_json::Value,
) -> Result<JsonRpcResponse> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("tools/call missing 'name'"))?;
    let args = params.get("arguments").cloned().unwrap_or_default();

    let text = match name {
        "create_entities" => {
            let p: CreateEntitiesParams = serde_json::from_value(args)?;
            db::mcp_create_entities(conn, p.entities).await?;
            "Entities created.".to_string()
        }
        "create_relations" => {
            let p: CreateRelationsParams = serde_json::from_value(args)?;
            db::mcp_create_relations(conn, p.relations).await?;
            "Relations created.".to_string()
        }
        "add_observations" => {
            let p: AddObservationsParams = serde_json::from_value(args)?;
            db::mcp_add_observations(conn, p.observations).await?;
            "Observations added.".to_string()
        }
        "delete_entities" => {
            let p: DeleteEntitiesParams = serde_json::from_value(args)?;
            db::mcp_delete_entities(conn, p.entity_names).await?;
            "Entities deleted.".to_string()
        }
        "delete_observations" => {
            let p: DeleteObservationsParams = serde_json::from_value(args)?;
            db::mcp_delete_observations(conn, p.deletions).await?;
            "Observations deleted.".to_string()
        }
        "delete_relations" => {
            let p: DeleteRelationsParams = serde_json::from_value(args)?;
            db::mcp_delete_relations(conn, p.relations).await?;
            "Relations deleted.".to_string()
        }
        "read_graph" => {
            let graph = db::mcp_read_graph(conn).await?;
            serde_json::to_string(&graph)?
        }
        "search_nodes" => {
            let p: SearchNodesParams = serde_json::from_value(args)?;
            let graph = db::mcp_search_nodes(conn, &p.query).await?;
            serde_json::to_string(&graph)?
        }
        "open_nodes" => {
            let p: OpenNodesParams = serde_json::from_value(args)?;
            let graph = db::mcp_open_nodes(conn, p.names).await?;
            serde_json::to_string(&graph)?
        }
        unknown => {
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(serde_json::json!({
                    "code": -32601,
                    "message": format!("Unknown tool: {}", unknown)
                })),
            });
        }
    };

    Ok(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": false
        })),
        error: None,
    })
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn noop(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: Some(serde_json::Value::Null),
        error: None,
    }
}

fn method_not_found(id: serde_json::Value, method: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id,
        result: None,
        error: Some(serde_json::json!({
            "code": -32601,
            "message": format!("Method not found: {}", method)
        })),
    }
}
