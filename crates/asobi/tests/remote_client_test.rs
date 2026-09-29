use std::process::{Command, Output};
use tempfile::tempdir;

fn asobi() -> &'static str {
    env!("CARGO_BIN_EXE_asobi")
}

fn run_cli(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new(asobi());
    command
        .args(args)
        .current_dir(dir)
        .env_remove("ASOBI_HOME")
        .env_remove("ASOBI_REMOTE")
        .env_remove("ASOBI_GRAPH");
    for (key, value) in envs {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_http_body_is_accepted_for_an_empty_request_operation() {
    use asobi_server::{App, BoundServer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let dir = tempdir().unwrap();
    let server = BoundServer::bind(
        std::sync::Arc::new(App::new(dir.path().to_path_buf())),
        "127.0.0.1:0".parse().unwrap(),
    )
    .await
    .unwrap();
    let mut stream = tokio::net::TcpStream::connect(server.local_addr)
        .await
        .unwrap();
    stream
        .write_all(
            b"POST /v3/graphs/empty-body/maintenance.stats HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8_lossy(&response);
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"entities\":0"), "{response}");
    server.shutdown().await;
}

#[cfg(not(feature = "remote"))]
#[test]
fn configured_remote_without_feature_fails_before_touching_a_graph() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("asobi.toml"),
        "remote = 'http://127.0.0.1:1'\ngraph = 'remote-test'\n",
    )
    .unwrap();
    let db = dir.path().join("should-not-exist.db");
    let output = run_cli(
        dir.path(),
        &["stats"],
        &[("ASOBI_DATABASE_URL", db.to_str().unwrap())],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("this asobi was built without remote support"),
        "{stderr}"
    );
    assert!(!db.exists(), "must fail before opening local graph");
}

#[test]
fn context_show_and_local_version_do_not_open_a_graph() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("asobi.toml"),
        "remote = 'http://127.0.0.1:1'\ngraph = 'file-graph'\n",
    )
    .unwrap();
    let db = dir.path().join("not-opened.db");
    let envs = [("ASOBI_DATABASE_URL", db.to_str().unwrap())];
    let context = run_cli(dir.path(), &["--json", "context", "show"], &envs);
    assert!(context.status.success());
    let value: serde_json::Value = serde_json::from_slice(&context.stdout).unwrap();
    assert_eq!(value["mode"], "remote");
    assert_eq!(value["graph"], "file-graph");
    assert!(value["source"].as_str().unwrap().ends_with("asobi.toml"));
    let local = run_cli(
        dir.path(),
        &["--json", "--local-graph", "context", "show"],
        &envs,
    );
    let value: serde_json::Value = serde_json::from_slice(&local.stdout).unwrap();
    assert_eq!(value["mode"], "local");
    assert_eq!(value["source"], "--local-graph");
    let secret = run_cli(
        dir.path(),
        &["--json", "context", "show"],
        &[(
            "ASOBI_REMOTE",
            "http://user:secret@127.0.0.1:1?token=secret",
        )],
    );
    let value: serde_json::Value = serde_json::from_slice(&secret.stdout).unwrap();
    assert_eq!(value["endpoint"], "<redacted>");
    assert!(!String::from_utf8_lossy(&secret.stdout).contains("secret"));
    let version = run_cli(dir.path(), &["--json", "--local-graph", "version"], &envs);
    assert!(version.status.success());
    let value: serde_json::Value = serde_json::from_slice(&version.stdout).unwrap();
    assert_eq!(value["serverVersion"], "not applicable");
    assert!(!db.exists());
}

#[cfg(feature = "remote")]
mod remote {
    use super::*;
    use asobi_core::api::{GraphStore, MaintenanceStore};
    use asobi_server::{App, BoundServer};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn command(dir: &std::path::Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
        run_cli(dir, args, envs)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn remote_cli_uses_server_and_local_filesystem_is_untouched() {
        let data = tempdir().unwrap();
        let server = BoundServer::bind(
            Arc::new(App::new(data.path().to_path_buf())),
            "127.0.0.1:0".parse().unwrap(),
        )
        .await
        .unwrap();
        let workspace = tempdir().unwrap();
        std::fs::write(
            workspace.path().join("asobi.toml"),
            "remote = 'http://127.0.0.1:1'\ngraph = 'file-graph'\n",
        )
        .unwrap();
        let local_db = workspace.path().join("local.db");
        let output = command(
            workspace.path(),
            &["new", "remote:one", "task", "--obs", "from server"],
            &[
                ("ASOBI_REMOTE", &format!("http://{}", server.local_addr)),
                ("ASOBI_GRAPH", "client-graph"),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            data.path().join("client-graph.db").exists(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !local_db.exists(),
            "remote selection must not open local db"
        );
        let local_view = asobi_storage::SqliteStore::open_at(&data.path().join("client-graph.db"))
            .await
            .unwrap();
        let graph = local_view.read_graph().await.unwrap();
        assert_eq!(graph.entities[0].name, "remote:one");
        let envs = [
            ("ASOBI_REMOTE", format!("http://{}", server.local_addr)),
            ("ASOBI_GRAPH", "client-graph".into()),
        ];
        let borrowed: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let version = command(workspace.path(), &["--json", "version"], &borrowed);
        assert!(version.status.success());
        let version: serde_json::Value = serde_json::from_slice(&version.stdout).unwrap();
        assert_eq!(version["serverVersion"], env!("CARGO_PKG_VERSION"));
        let info = command(workspace.path(), &["--json", "info"], &borrowed);
        assert!(info.status.success());
        let info: serde_json::Value = serde_json::from_slice(&info.stdout).unwrap();
        assert_eq!(info["mode"], "remote");
        assert_eq!(info["pathOwner"], "server");
        assert_eq!(info["graph"], "client-graph");

        // A refusal response maps back to the protocol's ApiError variant.
        let remote = asobi::storage::RemoteStore::new(
            format!("http://{}", server.local_addr),
            "client-graph".into(),
        )
        .unwrap();
        assert!(matches!(
            remote.reset().await,
            Err(asobi_core::api::ApiError::Unsupported(_))
        ));
        server.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn closed_port_fails_closed_for_reads_and_writes() {
        let workspace = tempdir().unwrap();
        let local_db = workspace.path().join("fallback.db");
        let remote = closed_port();
        let start = std::time::Instant::now();
        let output = command(
            workspace.path(),
            &["stats", "--json"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_GRAPH", "fallback-graph"),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("remote Asobi server unavailable")
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
        assert!(!local_db.exists());

        let next = command(
            workspace.path(),
            &["new", "offline:one", "task"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(!next.status.success());
        assert!(String::from_utf8_lossy(&next.stderr).contains("remote Asobi server unavailable"));
        assert!(!local_db.exists());

        let explicit = command(
            workspace.path(),
            &["--local-graph", "stats", "--json"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(explicit.status.success());
        assert!(local_db.exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn first_gateway_failure_fails_closed_for_all_gateway_statuses() {
        for status in [502, 503, 504] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                let body = "upstream unavailable";
                let response = format!(
                    "HTTP/1.1 {status} Gateway Error\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let workspace = tempdir().unwrap();
            let local_db = workspace.path().join("gateway-fallback.db");
            let remote = format!("http://{addr}");
            let output = command(
                workspace.path(),
                &["stats", "--json"],
                &[
                    ("ASOBI_REMOTE", &remote),
                    ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
                ],
            );
            assert!(!output.status.success(), "HTTP {status}: {:?}", output);
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("remote Asobi server unavailable")
            );
            assert!(
                !local_db.exists(),
                "HTTP {status} must not select local storage"
            );
            server.await.unwrap();
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn first_call_timeout_fails_closed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        });
        let workspace = tempdir().unwrap();
        let local_db = workspace.path().join("timeout-fallback.db");
        let remote = format!("http://{addr}");
        let started = std::time::Instant::now();
        let output = command(
            workspace.path(),
            &["stats", "--json"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(!output.status.success(), "{:?}", output);
        assert!(started.elapsed() >= std::time::Duration::from_secs(1));
        assert!(started.elapsed() < std::time::Duration::from_secs(3));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("remote Asobi server unavailable")
        );
        assert!(!local_db.exists());
        server.abort();
    }

    fn closed_port() -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn non_protocol_response_reports_api_v3_error_without_fallback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            let body = "not a v3 response";
            let response = format!(
                "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let workspace = tempdir().unwrap();
        let local_db = workspace.path().join("must-not-exist.db");
        let remote = format!("http://{addr}");
        let output = command(
            workspace.path(),
            &["stats"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(
            !output.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("server does not speak API v3"), "{stderr}");
        assert!(!local_db.exists());
        server_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failure_after_successful_remote_call_does_not_fall_back_mid_command() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let _ = first.read(&mut request).await.unwrap();
            let success = br#"{"databasePath":"remote","journalMode":"wal","schemaVersion":9}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                success.len(),
                String::from_utf8_lossy(success)
            );
            first.write_all(response.as_bytes()).await.unwrap();
            drop(first);
            // The JSON `new` command reads the created node next. Accept its
            // request and disappear, after the first remote call succeeded.
            let (_second, _) = listener.accept().await.unwrap();
        });
        let workspace = tempdir().unwrap();
        let local_db = workspace.path().join("must-not-fallback.db");
        let remote = format!("http://{addr}");
        let output = command(
            workspace.path(),
            &["--json", "new", "remote:one", "task"],
            &[
                ("ASOBI_REMOTE", &remote),
                ("ASOBI_DATABASE_URL", local_db.to_str().unwrap()),
            ],
        );
        assert!(
            !output.status.success(),
            "a later transport failure must fail the command"
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("remote server request failed")
                || stderr.contains("remote server request failed")
                || stdout.contains("server disconnected"),
            "output: {output:?}"
        );
        assert!(
            !stderr.contains("writes will not reach the server"),
            "no fallback warning after success: {stderr}"
        );
        assert!(!local_db.exists(), "the command must not switch storage");
        server_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn one_client_reuses_connection_for_multiple_calls() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(AtomicUsize::new(0));
        let accepted_task = accepted.clone();
        let requests_task = requests.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done_task = done.clone();
        let task = tokio::spawn(async move {
            while !done_task.load(Ordering::SeqCst) {
                let Ok(Ok((mut stream, _))) =
                    tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                        .await
                else {
                    break;
                };
                accepted_task.fetch_add(1, Ordering::SeqCst);
                let requests = requests_task.clone();
                tokio::spawn(async move {
                    loop {
                        let mut request = Vec::new();
                        let mut byte = [0u8; 1];
                        while stream.read_exact(&mut byte).await.is_ok() {
                            request.push(byte[0]);
                            if request.ends_with(b"\r\n\r\n") {
                                break;
                            }
                        }
                        if request.is_empty() {
                            break;
                        }
                        let headers = String::from_utf8_lossy(&request).to_ascii_lowercase();
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .and_then(|n| n.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        let mut body = vec![0; length];
                        if stream.read_exact(&mut body).await.is_err() {
                            break;
                        }
                        requests.fetch_add(1, Ordering::SeqCst);
                        let path = headers.lines().next().unwrap_or("");
                        let response_body = if path.contains("maintenance.location") {
                            r#"{"databasePath":"remote","journalMode":"wal","schemaVersion":9}"#
                        } else {
                            r#"{"entities":0,"relations":0,"observations":0}"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            response_body.len(),
                            response_body
                        );
                        if stream.write_all(response.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        let workspace = tempdir().unwrap();
        let remote = format!("http://{addr}");
        let db_path = workspace.path().join("unused-local.db");
        let workspace_path = workspace.path().to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            let mut cmd = Command::new(asobi());
            cmd.args(["stats", "--json"])
                .current_dir(workspace_path)
                .env_remove("ASOBI_HOME")
                .env_remove("ASOBI_REMOTE")
                .env_remove("ASOBI_GRAPH")
                .env("ASOBI_REMOTE", remote)
                .env("ASOBI_GRAPH", "g")
                .env("ASOBI_DATABASE_URL", db_path);
            cmd.output().unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
        assert_eq!(requests.load(Ordering::SeqCst), 2, "location + stats");
        done.store(true, Ordering::SeqCst);
        task.abort();
    }
}
