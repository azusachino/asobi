//! The background sweep: hourly, over every graph the server holds, plus any
//! other `*.db` graph file in the data directory.
//!
//! Failures are logged and skipped, never propagated: the sweep is hygiene,
//! and the server must keep serving (ADR 0005).

use std::sync::Arc;
use std::time::Duration;

use super::App;

/// Spawn the hourly sweep task. Tests trigger the sweep directly through
/// [`crate::GraphRegistry::sweep_all`] or [`crate::BoundServer::sweep_now`]
/// instead of waiting an hour.
pub(crate) fn spawn_background(app: Arc<App>, interval: Duration) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick fires immediately; skip it so a freshly started
        // server does not sweep an empty registry before serving anything.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            tracing::info!("background sweep starting");
            app.sweep_now().await;
        }
    })
}
