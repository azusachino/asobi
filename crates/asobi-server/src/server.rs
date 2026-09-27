//! The Asobi graph server: one long-lived process holding named graphs, one
//! SQLite file per graph in its data directory, answering the HTTP protocol
//! of ADR 0005.
//!
//! Structure: [`GraphRegistry`] owns the opened stores (one sqlx pool per
//! graph, opened lazily behind an async lock); [`Server`] binds the axum
//! router and serves it; the background sweep is a tokio task on a one-hour
//! interval that can also be triggered on demand for tests.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use asobi_core::protocol::{self, ProtocolError};
use axum::extract::{Path, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::json;

use crate::registry::GraphRegistry;
use crate::sweep;

/// Everything the handlers need, shared behind an `Arc`.
pub struct App {
    registry: GraphRegistry,
}

impl App {
    /// Serve the graphs in `data_dir`.
    pub fn new(data_dir: PathBuf) -> Self {
        App {
            registry: GraphRegistry::new(data_dir),
        }
    }

    /// The background sweep, on demand (tests; production uses the hourly
    /// task in `sweep::spawn_background`).
    pub async fn sweep_now(&self) {
        self.registry.sweep_all().await;
    }

    /// The bound router. Separate from [`Server::run`] so tests can hold the
    /// registry and trigger the sweep while the router serves real HTTP.
    fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route(
                "/v3/graphs/{graph}/{operation}",
                post(Self::call).fallback(Self::method_not_allowed),
            )
            .fallback(Self::no_such_path)
            .with_state(self.clone())
    }

    /// One RPC call: `POST /v3/graphs/<graph>/<operation>`, request body is
    /// the operation's request, 200 with the result JSON, or the dispatcher's
    /// error mapped to a status + `{kind, message}` body.
    async fn call(
        State(app): State<Arc<Self>>,
        Path((graph, operation)): Path<(String, String)>,
        body: axum::body::Bytes,
    ) -> Response {
        let start = Instant::now();
        let store = match app.registry.get_or_open(&graph).await {
            Ok(store) => store,
            Err(error) => return protocol_error(&error),
        };

        let result = protocol::dispatch(store.as_ref(), &graph, &operation, Some(&body)).await;
        let elapsed = start.elapsed();
        match result {
            Ok(value) => {
                tracing::info!(
                    graph = %graph,
                    operation = %operation,
                    duration = ?elapsed,
                    "request ok"
                );
                (StatusCode::OK, Json(value)).into_response()
            }
            Err(error) => {
                tracing::info!(
                    graph = %graph,
                    operation = %operation,
                    duration = ?elapsed,
                    error_kind = %error.kind,
                    status = error.status,
                    "request failed"
                );
                protocol_error(&error)
            }
        }
    }

    /// A known path hit with a non-POST verb: 405 `badRequest` (ADR 0005).
    /// Any other verb on an unknown path is a plain 404 from the router.
    async fn method_not_allowed() -> Response {
        let error = ProtocolError::protocol(405, "badRequest", "use POST");
        (StatusCode::METHOD_NOT_ALLOWED, Json(error.body())).into_response()
    }

    /// Any other path: plain 404, same shape as an unknown operation.
    async fn no_such_path(uri: Uri) -> Response {
        tracing::info!(path = %uri, "no such path");
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "kind": "notFound", "message": "no such path" })),
        )
            .into_response()
    }
}

fn protocol_error(error: &ProtocolError) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(error.body())).into_response()
}

/// A server bound to a socket, serving until shut down.
pub struct BoundServer {
    /// The actual address (useful when bound to port 0, as tests do).
    pub local_addr: SocketAddr,
    registry: Arc<App>,
    shutdown: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl BoundServer {
    /// Bind `app` to `addr` and serve in the background.
    pub async fn bind(app: Arc<App>, addr: SocketAddr) -> anyhow::Result<BoundServer> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let (shutdown, mut shutdown_rx) = tokio::sync::watch::channel(false);
        let router = app.router();
        let task = tokio::spawn(async move {
            let server = axum::serve(listener, router).with_graceful_shutdown(async move {
                let _ = shutdown_rx.changed().await;
            });
            if let Err(error) = server.await {
                tracing::error!(?error, "server task failed");
            }
        });
        Ok(BoundServer {
            local_addr,
            registry: app,
            shutdown,
            task,
        })
    }

    /// The address requests go to, as a `host:port` string.
    pub fn host(&self) -> String {
        self.local_addr.to_string()
    }

    /// Run the background sweep right now (tests; production uses the
    /// one-hour interval task).
    pub async fn sweep_now(&self) {
        self.registry.sweep_now().await;
    }

    /// Stop serving and wait for the task to finish.
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

/// Bind and serve until shutdown, on the multi-threaded runtime the caller
/// builds. Used by the binary; swept hourly in the background.
pub async fn run(data_dir: PathBuf, addr: SocketAddr) -> anyhow::Result<()> {
    let app = Arc::new(App::new(data_dir));
    sweep::spawn_background(app.clone(), std::time::Duration::from_secs(60 * 60));
    let server = BoundServer::bind(app, addr).await?;
    tracing::info!(addr = %server.local_addr, "asobi-server listening");
    server.task.await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}
