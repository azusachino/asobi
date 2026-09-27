//! The graph registry: graph name validation, lazily opened stores, and the
//! store sweep.
//!
//! One sqlx pool per graph (ADR 0008); requests are served concurrently from
//! those pools while SQLite itself serialises writes through WAL and the busy
//! timeout. The registry lock is only taken to look up or open a store, never
//! for the duration of a request.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asobi_core::protocol::ProtocolError;
use asobi_storage::SqliteStore;
use tokio::sync::Mutex;

/// Graph names become files on the server, so they are restricted to
/// `^[a-z0-9-]+$` -- anything else is rejected before a path is ever built
/// (ADR 0005). Hand-checked rather than regex'd: the shape is one class of
/// characters and non-empty.
pub fn is_valid_graph_name(graph: &str) -> bool {
    !graph.is_empty()
        && graph
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Opened stores, keyed by graph name, behind an async lock (WP4). The lock
/// is held only across the map lookup / store open, never across a request's
/// dispatch, so requests to different graphs run concurrently.
pub struct GraphRegistry {
    data_dir: PathBuf,
    stores: Mutex<HashMap<String, Arc<SqliteStore>>>,
}

impl GraphRegistry {
    pub fn new(data_dir: PathBuf) -> Self {
        GraphRegistry {
            data_dir,
            stores: Mutex::new(HashMap::new()),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The file backing a graph: `<data_dir>/<name>.db`.
    pub fn graph_path(&self, graph: &str) -> PathBuf {
        self.data_dir.join(format!("{graph}.db"))
    }

    /// The store for a graph, opened lazily; an unknown valid graph name
    /// creates `<data_dir>/<name>.db` on first use. An invalid name is
    /// rejected with 422 `invalid` before any path is touched, so no file is
    /// created for it (ADR 0005).
    pub async fn get_or_open(&self, graph: &str) -> Result<Arc<SqliteStore>, ProtocolError> {
        if !is_valid_graph_name(graph) {
            return Err(invalid_graph_error(graph));
        }
        let mut stores = self.stores.lock().await;
        if let Some(store) = stores.get(graph) {
            return Ok(store.clone());
        }
        let path = self.graph_path(graph);
        let store = Arc::new(
            SqliteStore::open_at(&path)
                .await
                .map_err(|error| ProtocolError::protocol(503, "unavailable", error.to_string()))?,
        );
        stores.insert(graph.to_string(), store.clone());
        Ok(store)
    }

    /// Sweep every graph: the ones already opened, plus any other `*.db`
    /// file in the data directory (a graph another process may have created).
    /// A failure on one graph is logged and skipped, never propagated: the
    /// sweep is hygiene, and the server must keep serving (ADR 0005).
    pub async fn sweep_all(&self) {
        let mut graphs: Vec<String> = self.stores.lock().await.keys().cloned().collect();
        if let Ok(mut dir) = tokio::fs::read_dir(&self.data_dir).await {
            while let Ok(Some(entry)) = dir.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(graph) = name.strip_suffix(".db")
                    && is_valid_graph_name(graph)
                    && !graphs.iter().any(|g| g == graph)
                {
                    graphs.push(graph.to_string());
                }
            }
        }
        for graph in graphs {
            match self.get_or_open(&graph).await {
                Ok(store) => {
                    if let Err(error) = store.sweep().await {
                        tracing::warn!(graph = %graph, ?error, "sweep failed");
                    }
                }
                Err(error) => tracing::warn!(graph = %graph, ?error, "sweep skipped graph"),
            }
        }
    }
}

fn invalid_graph_error(graph: &str) -> ProtocolError {
    ProtocolError::protocol(
        422,
        "invalid",
        format!("invalid graph name {graph:?}: names match ^[a-z0-9-]+$"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_name_validation() {
        assert!(is_valid_graph_name("asobi"));
        assert!(is_valid_graph_name("work-laptop"));
        assert!(is_valid_graph_name("a1-2"));
        // Traversal, emptiness, case, and separators are all rejected.
        assert!(!is_valid_graph_name("../x"));
        assert!(!is_valid_graph_name(".."));
        assert!(!is_valid_graph_name(""));
        assert!(!is_valid_graph_name("A"));
        assert!(!is_valid_graph_name("my_graph"));
        assert!(!is_valid_graph_name("graph/db"));
    }
}
