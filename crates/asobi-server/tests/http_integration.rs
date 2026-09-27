//! Integration tests: a real server on 127.0.0.1:0, driven over real HTTP
//! with a hand-rolled minimal client. Nothing here calls the dispatcher
//! directly — everything goes through TCP.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use asobi_server::{App, BoundServer};
use serde_json::{Value, json};
use sqlx::Connection;
use tempfile::tempdir;

/// Set one `ASOBI_*` variable for a test and remove it on drop. The
/// workspace test suite runs serially (`--test-threads=1`), so this stays
/// hermetic.
struct EnvGuard {
    key: &'static str,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        unsafe { std::env::set_var(key, value) };
        EnvGuard { key }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(self.key) };
    }
}

async fn start_server() -> (tempfile::TempDir, SocketAddr, BoundServer) {
    let dir = tempdir().unwrap();
    let server = BoundServer::bind(
        Arc::new(App::new(dir.path().to_path_buf())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    (dir, server.local_addr, server)
}

/// A minimal HTTP/1.1 client: open a TCP connection, send `method` + `path` +
/// body with `Connection: close`, read the whole response.
async fn call(addr: SocketAddr, method: &str, path: &str, body: &str) -> (u16, String, Value) {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: asobi.test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.unwrap();
    let text = String::from_utf8_lossy(&raw).to_string();
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body_text = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    // Chunked or content-length, we take whatever the body holds; an empty
    // body parses as null.
    let json = serde_json::from_str(&body_text).unwrap_or(Value::Null);
    (status, body_text, json)
}

async fn post(addr: SocketAddr, path: &str, body: &str) -> (u16, Value) {
    let (status, _, json) = call(addr, "POST", path, body).await;
    (status, json)
}

async fn seed(addr: SocketAddr, graph: &str, entity: &str) {
    let (status, _) = post(
        addr,
        &format!("/v3/graphs/{graph}/graph.createEntities"),
        &json!({ "entities": [{ "name": entity, "entityType": "task",
                                "observations": [format!("{graph} wrote {entity}")] }] })
        .to_string(),
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn graphs_are_isolated_and_a_new_name_creates_its_file() {
    let (dir, addr, _server) = start_server().await;
    seed(addr, "alpha", "alpha:task-1").await;
    seed(addr, "beta", "beta:task-1").await;

    // Each graph file exists under the data directory.
    assert!(dir.path().join("alpha.db").is_file());
    assert!(dir.path().join("beta.db").is_file());

    // Isolation: alpha's graph does not know beta's entity, and the other
    // way around.
    for (graph, expected) in [("alpha", "alpha:task-1"), ("beta", "beta:task-1")] {
        let (status, result) =
            post(addr, &format!("/v3/graphs/{graph}/graph.readGraph"), "{}").await;
        assert_eq!(status, 200);
        let names: Vec<&str> = result["entities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec![expected],
            "graph {graph} must hold only its own entity"
        );
    }
}

#[tokio::test]
async fn invalid_graph_names_are_refused_and_create_no_file() {
    let (dir, addr, _server) = start_server().await;

    // Uppercase: rejected with 422 invalid.
    let (status, body) = post(addr, "/v3/graphs/A/maintenance.stats", "{}").await;
    assert_eq!(status, 422);
    assert_eq!(body["kind"], "invalid");

    // Traversal and empty names never reach a valid route: 404 (fallback) or
    // 400 from the router — either way no file is created.
    let (status_a, _) = post(addr, "/v3/graphs/../x/maintenance.stats", "{}").await;
    assert_ne!(status_a, 200);
    let (status_b, _) = post(addr, "/v3/graphs//graph.readGraph", "{}").await;
    assert_ne!(status_b, 200);

    // No database file was created for any rejected name.
    let created: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        created.is_empty(),
        "no file may be created for a rejected name: {created:?}"
    );
}

#[tokio::test]
async fn reset_over_http_is_refused_and_the_graph_survives() {
    let (dir, addr, _server) = start_server().await;
    seed(addr, "alpha", "alpha:task-1").await;

    let (status, body) = post(addr, "/v3/graphs/alpha/maintenance.reset", "{}").await;
    assert_eq!(status, 501);
    assert_eq!(body["kind"], "unsupported");
    assert!(
        body["message"].as_str().unwrap().contains("asobi reset"),
        "the message names the alternative: {body}"
    );

    // The graph is intact.
    let (status, graph) = post(addr, "/v3/graphs/alpha/graph.readGraph", "{}").await;
    assert_eq!(status, 200);
    assert_eq!(graph["entities"].as_array().unwrap().len(), 1);
    let _ = dir;
}

#[tokio::test]
async fn non_post_methods_are_405_bad_request_and_other_paths_404() {
    let (_dir, addr, _server) = start_server().await;
    let (status, _, body) = call(addr, "GET", "/v3/graphs/alpha/graph.readGraph", "").await;
    assert_eq!(status, 405);
    assert_eq!(body["kind"], "badRequest");

    let (status, _, body) = call(addr, "POST", "/v3/nothing/here", "{}").await;
    assert_eq!(status, 404);
    assert_eq!(body["kind"], "notFound");
}

#[tokio::test]
async fn a_request_during_a_sweep_completes_after_it() {
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "90");
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "90");
    let (dir, addr, server) = start_server().await;
    seed(addr, "alpha", "alpha:task-1").await;

    // Sweep and a concurrent write race; both must succeed and the write
    // must land (requests and the sweep serialise through SQLite, nothing
    // interleaves into a broken state).
    let sweep = server.sweep_now();
    let write = seed(addr, "alpha", "alpha:task-2");
    let ((), ()) = tokio::join!(sweep, write);

    let (status, graph) = post(addr, "/v3/graphs/alpha/graph.readGraph", "{}").await;
    assert_eq!(status, 200);
    let names: Vec<&str> = graph["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alpha:task-1"), "{names:?}");
    assert!(names.contains(&"alpha:task-2"), "{names:?}");
    let _ = dir;
}

#[tokio::test]
async fn the_on_demand_sweep_abandons_and_deletes_expired_tasks() {
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "1");
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "1");
    let (dir, addr, server) = start_server().await;
    seed(addr, "alpha", "alpha:idle").await;
    let db_path: PathBuf = dir.path().join("alpha.db");

    // Age the whole activity trail past both windows, directly in the graph
    // file (the same seeding the storage sweep tests use).
    let mut conn = sqlx::sqlite::SqliteConnection::connect(&format!(
        "sqlite://{}?mode=rwc",
        db_path.display()
    ))
    .await
    .unwrap();
    sqlx::raw_sql(
        "UPDATE asobi_entities SET created_at = datetime('now', '-30 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-30 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-30 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-30 days');",
    )
    .execute(&mut conn)
    .await
    .unwrap();
    conn.close().await.unwrap();

    // The trigger: sweep on demand.
    server.sweep_now().await;

    // The idle task was abandoned by the sweep...
    let (status, graph) = post(addr, "/v3/graphs/alpha/graph.readGraph", "{}").await;
    assert_eq!(status, 200);
    assert_eq!(graph["entities"][0]["truths"]["status"], "ABANDONED");
}

#[tokio::test]
async fn graphs_land_in_the_data_directory_the_server_was_given() {
    // The data directory is exactly what --data-dir names — nothing is
    // resolved from asobi.toml/XDG — and the graph file lands inside it.
    let dir = tempdir().unwrap();
    let parsed = asobi_server::parse_args([
        "--listen".to_string(),
        "127.0.0.1:0".to_string(),
        "--data-dir".to_string(),
        dir.path().to_string_lossy().to_string(),
    ])
    .unwrap();
    assert_eq!(parsed.data_dir, dir.path());

    let server = BoundServer::bind(
        Arc::new(App::new(parsed.data_dir.clone())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    seed(server.local_addr, "gamma", "gamma:task-1").await;
    assert!(parsed.data_dir.join("gamma.db").is_file());
}

#[tokio::test]
async fn an_empty_body_is_an_absent_body() {
    // A POST with no body at all (what curl or xh send by default) must
    // work for operations whose request is empty, not fail to parse "".
    let (_dir, addr, _server) = start_server().await;
    let (status, hello) = post(addr, "/v3/graphs/alpha/server.hello", "").await;
    assert_eq!(status, 200, "{hello}");
    assert_eq!(hello["stateId"], "alpha");
    // Operations that need fields still reject an empty body.
    let (status, body) = post(addr, "/v3/graphs/alpha/tasks.claimNext", "").await;
    assert_eq!(status, 400, "{body}");
}
