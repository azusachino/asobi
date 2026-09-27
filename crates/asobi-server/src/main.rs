//! `asobi-server` — the Asobi graph server: named graphs under the server's
//! own data directory, served over HTTP on the tailnet (ADR 0005).

use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

/// Mirrors the CLI's timer (`crates/asobi/src/cli/runtime.rs`): the default
/// `tracing_subscriber` clock is UTC regardless of `TZ`, which left the
/// container's `TZ` env var with no visible effect on log timestamps.
#[derive(Debug, Clone, Copy, Default)]
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

fn main() -> anyhow::Result<()> {
    let args = asobi_server::parse_args(std::env::args().skip(1))?;

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_timer(LocalTimer)
        .init();

    // The server's data directory is its own (required argument, created if
    // missing): never the CLI's asobi.toml/XDG directory.
    std::fs::create_dir_all(&args.data_dir)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(asobi_server::run(args.data_dir, args.listen))
}
