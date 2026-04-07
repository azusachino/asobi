use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::info;

/// GracefulShutdown handles signal reception (Ctrl+C, SIGTERM)
/// and coordinates shutdown across the application using a CancellationToken.
pub struct GracefulShutdown {
    token: CancellationToken,
}

impl Default for GracefulShutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl GracefulShutdown {
    /// Creates a new GracefulShutdown instance with a new CancellationToken.
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    /// Returns a clone of the internal CancellationToken.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Waits for a termination signal (SIGINT or SIGTERM) and cancels the token.
    pub async fn wait_for_signal(self) {
        let ctrl_c = signal::ctrl_c();

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {
                info!("received Ctrl+C, shutting down...");
            }
            _ = terminate => {
                info!("received SIGTERM, shutting down...");
            }
        }

        self.token.cancel();
    }
}
