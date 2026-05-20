# MCP Knowledge Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend Rosemary with an MCP server implementing a persistent Knowledge Graph with XDG-compliant tiered storage (Hot Tier in libSQL, Cold Tier in Markdown).

**Architecture:** Hybrid tiered storage model. Fast writes/relations go to libSQL (Hot Tier). Durable, human-editable archival lives in Markdown (Cold Tier). Vector search (LanceDB) covers both. Standard MCP stdio-based server for agent interaction.

**Tech Stack:** Rust (Edition 2024), libSQL, LanceDB, Tokio, XDG-base directories, Serde.

---

### Task 1: Path Management (XDG)

**Files:**
- Create: `src/paths.rs`
- Modify: `src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `directories` crate to Cargo.toml**
```toml
[dependencies]
# ...
directories = "6"
```

- [ ] **Step 2: Implement `src/paths.rs`**
```rust
use directories::ProjectDirs;
use std::path::PathBuf;
use std::env;

pub struct RosemaryPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub kb_dir: PathBuf,
}

impl RosemaryPaths {
    pub fn resolve() -> Self {
        let home = env::var("ROSEMARY_HOME").map(PathBuf::from).ok();
        
        let proj_dirs = ProjectDirs::from("me", "azusachino", "rosemary");
        
        let data_dir = home.clone().unwrap_or_else(|| {
            proj_dirs.as_ref().map(|d| d.data_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".rosemary/data"))
        });

        let config_dir = home.clone().unwrap_or_else(|| {
            proj_dirs.as_ref().map(|d| d.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".rosemary/config"))
        });

        let kb_dir = home.unwrap_or_else(|| {
            proj_dirs.as_ref().map(|d| d.data_dir().join("topics"))
                .unwrap_or_else(|| PathBuf::from("kb/topics"))
        });

        Self { data_dir, config_dir, kb_dir }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("rosemary.db")
    }
}
```

- [ ] **Step 3: Register `paths` module in `src/lib.rs`**
```rust
pub mod paths;
// ...
```

- [ ] **Step 4: Commit**
```bash
git add Cargo.toml src/paths.rs src/lib.rs
git commit -m "feat: add XDG path resolution foundation"
```

---

### Task 2: libSQL Schema Expansion (Graph Primitives)

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Update `init_db` to use `RosemaryPaths` and add graph tables**
```rust
pub async fn init_db() -> Result<(Database, Connection)> {
    let paths = crate::paths::RosemaryPaths::resolve();
    std::fs::create_dir_all(&paths.data_dir)?;
    
    let db_url = env::var("DATABASE_URL")
        .unwrap_or_else(|| paths.db_path().to_str().unwrap().to_string());
    
    let db = Builder::new_local(&db_url).build().await?;
    let conn = db.connect()?;

    // ... existing topics/sessions tables ...

    // Update mcp_entities table to match spec
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_entities (
            name        TEXT PRIMARY KEY,
            entity_type TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
        (),
    ).await?;

    // Create mcp_observations table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_observations (
            id          TEXT PRIMARY KEY,
            entity_name TEXT NOT NULL,
            content     TEXT NOT NULL,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (entity_name) REFERENCES mcp_entities(name) ON DELETE CASCADE
        )",
        (),
    ).await?;

    // Create mcp_relations table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS mcp_relations (
            from_entity   TEXT NOT NULL,
            to_entity     TEXT NOT NULL,
            relation_type TEXT NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (from_entity, to_entity, relation_type),
            FOREIGN KEY (from_entity) REFERENCES mcp_entities(name) ON DELETE CASCADE,
            FOREIGN KEY (to_entity)   REFERENCES mcp_entities(name) ON DELETE CASCADE
        )",
        (),
    ).await?;

    Ok((db, conn))
}
```

- [ ] **Step 2: Run `cargo check` to verify migrations**
```bash
cargo check
```

- [ ] **Step 3: Commit**
```bash
git add src/db.rs
git commit -m "feat: add libSQL tables for Knowledge Graph (Hot Tier)"
```

---

### Task 3: MCP Protocol & Model Implementation

**Files:**
- Create: `src/mcp.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Define MCP JSON-RPC structures in `src/mcp.rs`**
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEntitiesParams { pub entities: Vec<EntityInput> }

#[derive(Debug, Deserialize)]
pub struct EntityInput {
    pub name: String,
    pub entityType: String,
    pub observations: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelationsParams { pub relations: Vec<RelationInput> }

#[derive(Debug, Deserialize)]
pub struct RelationInput {
    pub from: String,
    pub to: String,
    pub relationType: String,
}

#[derive(Debug, Deserialize)]
pub struct AddObservationsParams { pub observations: Vec<ObservationInput> }

#[derive(Debug, Deserialize)]
pub struct ObservationInput {
    pub entityName: String,
    pub contents: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchNodesParams { pub query: String }

#[derive(Debug, Deserialize)]
pub struct OpenNodesParams { pub names: Vec<String> }

#[derive(Debug, Deserialize)]
pub struct DeleteEntitiesParams { pub entityNames: Vec<String> }
```

- [ ] **Step 2: Register module in `src/lib.rs`**
```rust
pub mod mcp;
```

- [ ] **Step 3: Commit**
```bash
git add src/mcp.rs src/lib.rs
git commit -m "feat: add MCP protocol structures"
```

---

### Task 4: Implement Graph DB Operations

**Files:**
- Modify: `src/db.rs`

- [ ] **Step 1: Implement `create_entities` and `add_observations`**
```rust
pub async fn mcp_create_entities(conn: &Connection, entities: Vec<crate::mcp::EntityInput>) -> Result<()> {
    for ent in entities {
        conn.execute(
            "INSERT OR IGNORE INTO mcp_entities (name, entity_type) VALUES (?1, ?2)",
            libsql::params![ent.name, ent.entityType],
        ).await?;
        for obs in ent.observations {
            conn.execute(
                "INSERT INTO mcp_observations (id, entity_name, content) VALUES (?1, ?2, ?3)",
                libsql::params![uuid::Uuid::new_v4().to_string(), ent.name, obs],
            ).await?;
        }
    }
    Ok(())
}

pub async fn mcp_add_observations(conn: &Connection, observations: Vec<crate::mcp::ObservationInput>) -> Result<()> {
    for obs_batch in observations {
        for content in obs_batch.contents {
            conn.execute(
                "INSERT INTO mcp_observations (id, entity_name, content) VALUES (?1, ?2, ?3)",
                libsql::params![uuid::Uuid::new_v4().to_string(), obs_batch.entityName, content],
            ).await?;
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Implement `create_relations` and `delete_entities`**
```rust
pub async fn mcp_create_relations(conn: &Connection, relations: Vec<crate::mcp::RelationInput>) -> Result<()> {
    for rel in relations {
        conn.execute(
            "INSERT OR REPLACE INTO mcp_relations (from_entity, to_entity, relation_type) VALUES (?1, ?2, ?3)",
            libsql::params![rel.from, rel.to, rel.relationType],
        ).await?;
    }
    Ok(())
}

pub async fn mcp_delete_entities(conn: &Connection, names: Vec<String>) -> Result<()> {
    for name in names {
        conn.execute("DELETE FROM mcp_entities WHERE name = ?1", libsql::params![name]).await?;
    }
    Ok(())
}
```

- [ ] **Step 3: Implement `read_graph` and `search_nodes`**
Logic: `read_graph` returns all entities and relations. `search_nodes` uses FTS on observations + exact match on entity names.

- [ ] **Step 4: Commit**
```bash
git add src/db.rs
git commit -m "feat: implement Graph DB operations"
```

---

### Task 5: Implement MCP Stdio Server

**Files:**
- Modify: `src/mcp.rs`

- [ ] **Step 1: Implement the message handling loop**
```rust
use crate::db;
use libsql::Connection;

pub async fn run_server(conn: Connection) -> Result<()> {
    use std::io::{self, BufRead};
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(_) => { line.clear(); continue; }
        };
        
        let response = handle_request(&conn, req).await?;
        println!("{}", serde_json::to_string(&response)?);
        line.clear();
    }
    Ok(())
}

async fn handle_request(conn: &Connection, req: JsonRpcRequest) -> Result<JsonRpcResponse> {
    // Dispatch to db::mcp_* functions based on req.method
    // Return result or error
}
```

- [ ] **Step 2: Commit**
```bash
git add src/mcp.rs
git commit -m "feat: implement MCP stdio server loop"
```

---

### Task 3: Hot-to-Cold Compaction Logic

**Files:**
- Modify: `src/compact.rs`

- [ ] **Step 1: Implement `sync_graph_to_markdown`**
Logic:
1. For each `mcp_entity`:
   * Fetch all observations and relations.
   * Format as Markdown with YAML frontmatter.
   * Save to `kb/topics/<slug>.md`.
2. Trigger `ingest_file` on the updated files to update Vector/FTS tier.

- [ ] **Step 2: Commit**
```bash
git add src/compact.rs
git commit -m "feat: implement graph-to-markdown compaction"
```

---

### Task 6: CLI Refactor & Final Verification

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Refactor `main.rs` to use `clap` subcommands**
```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "rosemary", version = "0.2.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the MCP server
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Knowledge Base operations
    Kb {
        #[command(subcommand)]
        action: KbAction,
    },
}

#[derive(Subcommand)]
enum McpAction { Start }

#[derive(Subcommand)]
enum KbAction {
    Ingest { path: String },
    Query { query: String },
    Compact,
}
```

- [ ] **Step 2: Verify everything**
```bash
cargo run -- kb ingest kb/topics
cargo run -- kb compact
echo '{"jsonrpc":"2.0","id":1,"method":"read_graph"}' | cargo run -- mcp start
```

- [ ] **Step 3: Commit**
```bash
git add src/main.rs
git commit -m "feat: unified CLI entrypoint for KB and MCP"
```
