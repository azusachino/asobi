use rosemary::observability::init_tracing;
use rosemary::shutdown::GracefulShutdown;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    init_tracing();

    info!("starting graceful shutdown example...");

    // Create the shutdown manager
    let shutdown = GracefulShutdown::new();
    let token = shutdown.token();

    // Spawn a background worker
    let worker_handle = tokio::spawn(async move {
        info!("background worker started");
        let mut count = 0;

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("worker received shutdown signal, cleaning up...");
                    // Simulate cleanup
                    sleep(Duration::from_millis(500)).await;
                    info!("worker cleanup complete");
                    break;
                }
                _ = sleep(Duration::from_secs(1)) => {
                    count += 1;
                    info!("worker heartbeat: {}", count);
                }
            }
        }
    });

    // Wait for shutdown signal in the main task
    shutdown.wait_for_signal().await;

    info!("signal received, waiting for background worker to finish...");

    // Wait for the worker to finish
    if let Err(e) = worker_handle.await {
        warn!("worker task encountered an error during shutdown: {:?}", e);
    }

    info!("graceful shutdown complete");
    Ok(())
}
