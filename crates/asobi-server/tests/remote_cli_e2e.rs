use asobi_server::{App, BoundServer};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Barrier};
use tempfile::tempdir;

fn asobi_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_asobi") {
        return PathBuf::from(path);
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("asobi");
    assert!(
        path.is_file(),
        "build the remote-enabled CLI before running this suite: {}",
        path.display()
    );
    path
}

fn command(workspace: &Path, remote: Option<&str>, graph: &str, args: &[String]) -> Command {
    let local_db = workspace.join("local.db");
    let mut command = Command::new(asobi_bin());
    command
        .args(args)
        .current_dir(workspace)
        .env_remove("ASOBI_HOME")
        .env_remove("ASOBI_REMOTE")
        .env_remove("ASOBI_GRAPH")
        .env_remove("ASOBI_DATABASE_URL")
        .env_remove("ASOBI_ABANDON_DAYS")
        .env_remove("ASOBI_RETENTION_DAYS")
        .env("ASOBI_DATABASE_URL", local_db);
    if let Some(remote) = remote {
        command
            .env("ASOBI_REMOTE", remote)
            .env("ASOBI_GRAPH", graph);
    }
    command
}

async fn run_cli(workspace: &Path, remote: Option<&str>, graph: &str, args: &[&str]) -> Output {
    let workspace = workspace.to_path_buf();
    let remote = remote.map(str::to_string);
    let graph = graph.to_string();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    tokio::task::spawn_blocking(move || {
        command(&workspace, remote.as_deref(), &graph, &args)
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn graph_output(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

async fn start_server(data_dir: &Path) -> BoundServer {
    BoundServer::bind(
        Arc::new(App::new(data_dir.to_path_buf())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap()
}

fn remote(server: &BoundServer) -> String {
    format!("http://{}", server.local_addr)
}

#[tokio::test(flavor = "multi_thread")]
async fn two_workspaces_share_a_graph_and_a_third_graph_is_isolated() {
    let data = tempdir().unwrap();
    let server = start_server(data.path()).await;
    let remote = remote(&server);
    let device_a = tempdir().unwrap();
    let device_b = tempdir().unwrap();
    let device_c = tempdir().unwrap();

    let created = run_cli(
        device_a.path(),
        Some(&remote),
        "shared",
        &[
            "new",
            "shared:item",
            "concept",
            "--obs",
            "written by device A",
        ],
    )
    .await;
    assert!(created.status.success(), "{created:?}");
    assert!(!device_a.path().join("local.db").exists());

    let added = run_cli(
        device_b.path(),
        Some(&remote),
        "shared",
        &["obs", "shared:item", "written by device B"],
    )
    .await;
    assert!(added.status.success(), "{added:?}");
    let seen_by_a = graph_output(
        &run_cli(
            device_a.path(),
            Some(&remote),
            "shared",
            &["show", "shared:item"],
        )
        .await,
    );
    let entity = &seen_by_a["entities"][0];
    assert_eq!(entity["name"], "shared:item");
    assert!(
        entity["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "written by device A")
    );
    assert!(
        entity["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == "written by device B")
    );

    let other_graph =
        graph_output(&run_cli(device_c.path(), Some(&remote), "isolated", &["graph"]).await);
    assert!(other_graph["entities"].as_array().unwrap().is_empty());
    assert!(!device_b.path().join("local.db").exists());
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn outage_writes_stay_local_and_remote_mode_recovers_after_restart() {
    let data = tempdir().unwrap();
    let server = start_server(data.path()).await;
    let first_remote = remote(&server);
    let device = tempdir().unwrap();

    let seed = run_cli(
        device.path(),
        Some(&first_remote),
        "shared",
        &["new", "remote:seed", "task", "--obs", "server data"],
    )
    .await;
    assert!(seed.status.success(), "{seed:?}");
    server.shutdown().await;

    let outage = run_cli(
        device.path(),
        Some(&first_remote),
        "shared",
        &["new", "outage:local-only", "task", "--obs", "outage data"],
    )
    .await;
    assert!(outage.status.success(), "{outage:?}");
    assert!(String::from_utf8_lossy(&outage.stderr).contains("writes will not reach the server"));
    let local_db = device.path().join("local.db");
    assert!(
        local_db.is_file(),
        "fallback created the workspace-local graph"
    );
    let local_graph = graph_output(&run_cli(device.path(), None, "shared", &["graph"]).await);
    assert_eq!(local_graph["entities"][0]["name"], "outage:local-only");

    let server = start_server(data.path()).await;
    let recovered_remote = remote(&server);
    let recovered = run_cli(device.path(), Some(&recovered_remote), "shared", &["graph"]).await;
    let remote_graph = graph_output(&recovered);
    let remote_names: Vec<&str> = remote_graph["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| entity["name"].as_str().unwrap())
        .collect();
    assert_eq!(remote_names, vec!["remote:seed"]);
    assert!(
        !remote_names.contains(&"outage:local-only"),
        "outage write was not merged remotely"
    );
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn twelve_cli_processes_can_share_the_remote_graph_concurrently() {
    let data = tempdir().unwrap();
    let server = start_server(data.path()).await;
    let remote = remote(&server);
    let workspaces = tempdir().unwrap();
    let seed_workspace = workspaces.path().join("seed");
    std::fs::create_dir_all(&seed_workspace).unwrap();
    let created = run_cli(
        &seed_workspace,
        Some(&remote),
        "concurrency",
        &["new", "concurrent:item", "concept"],
    )
    .await;
    assert!(created.status.success(), "{created:?}");

    let process_count = 12;
    let barrier = Arc::new(Barrier::new(process_count));
    let bin = asobi_bin();
    let remote = Arc::new(remote);
    let root = workspaces.path().to_path_buf();
    let mut workers = Vec::new();
    for worker in 0..process_count {
        let barrier = barrier.clone();
        let bin = bin.clone();
        let remote = remote.clone();
        let root = root.clone();
        workers.push(std::thread::spawn(move || {
            let workspace = root.join(format!("device-{worker}"));
            std::fs::create_dir_all(&workspace).unwrap();
            barrier.wait();
            for iteration in 0..10 {
                let mut command = Command::new(&bin);
                command
                    .current_dir(&workspace)
                    .env_remove("ASOBI_HOME")
                    .env_remove("ASOBI_REMOTE")
                    .env_remove("ASOBI_GRAPH")
                    .env_remove("ASOBI_DATABASE_URL")
                    .env_remove("ASOBI_ABANDON_DAYS")
                    .env_remove("ASOBI_RETENTION_DAYS")
                    .env("ASOBI_REMOTE", remote.as_str())
                    .env("ASOBI_GRAPH", "concurrency")
                    .env("ASOBI_DATABASE_URL", workspace.join("local.db"));
                if worker % 2 == 0 {
                    command
                        .args(["obs", "concurrent:item"])
                        .arg(format!("worker {worker} iteration {iteration}"));
                } else if iteration % 2 == 0 {
                    command.args(["show", "concurrent:item"]);
                } else {
                    command
                        .args(["truth", "concurrent:item"])
                        .arg(format!("worker-{worker}"))
                        .arg(format!("value-{iteration}"));
                }
                let output = command.output().unwrap();
                assert!(
                    output.status.success(),
                    "remote worker {worker}, iteration {iteration} failed\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }));
    }
    tokio::task::spawn_blocking(move || {
        for worker in workers {
            worker.join().unwrap();
        }
    })
    .await
    .unwrap();

    let workspace = tempdir().unwrap();
    let graph =
        graph_output(&run_cli(workspace.path(), Some(&remote), "concurrency", &["graph"]).await);
    let entity = &graph["entities"][0];
    assert_eq!(entity["name"], "concurrent:item");
    assert_eq!(entity["observationCount"], 60);
    for worker in (1..process_count).step_by(2) {
        assert!(entity["truths"].get(format!("worker-{worker}")).is_some());
    }
    assert!(!workspaces.path().join("device-0/local.db").exists());
    server.shutdown().await;
}

struct EnvGuard(&'static str);

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        unsafe { std::env::set_var(key, value) };
        Self(key)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var(self.0) };
    }
}

async fn age_task(db: &Path) {
    use sqlx::Connection;
    let mut connection =
        sqlx::sqlite::SqliteConnection::connect(&format!("sqlite://{}?mode=rwc", db.display()))
            .await
            .unwrap();
    sqlx::raw_sql(
        "UPDATE asobi_entities SET created_at = datetime('now', '-30 days');
         UPDATE asobi_observations SET created_at = datetime('now', '-30 days');
         UPDATE asobi_truths SET updated_at = datetime('now', '-30 days');
         UPDATE asobi_entities SET last_activity = datetime('now', '-30 days');",
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_sweep_abandons_an_idle_task_visible_over_http() {
    let _abandon = EnvGuard::set("ASOBI_ABANDON_DAYS", "1");
    let _retention = EnvGuard::set("ASOBI_RETENTION_DAYS", "0");
    let data = tempdir().unwrap();
    let server = start_server(data.path()).await;
    let remote = remote(&server);
    let device = tempdir().unwrap();

    let created = run_cli(
        device.path(),
        Some(&remote),
        "abandonment",
        &["new", "task:idle", "task", "--obs", "waiting"],
    )
    .await;
    assert!(created.status.success(), "{created:?}");
    age_task(&data.path().join("abandonment.db")).await;
    server.sweep_now().await;

    let result = graph_output(
        &run_cli(
            device.path(),
            Some(&remote),
            "abandonment",
            &["show", "task:idle"],
        )
        .await,
    );
    assert_eq!(result["entities"][0]["truths"]["status"], "ABANDONED");
    assert!(
        result["entities"][0]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item.as_str()
                    .unwrap()
                    .contains("abandoned automatically after 1 idle days")
            })
    );
    server.shutdown().await;
}
