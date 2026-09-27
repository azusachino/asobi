//! `asobi-server` — the Asobi graph server: named graphs under the server's
//! own data directory, served over HTTP on the tailnet (ADR 0005).

fn main() -> anyhow::Result<()> {
    let args = asobi_server::parse_args(std::env::args().skip(1))?;

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // The server's data directory is its own (required argument, created if
    // missing): never the CLI's asobi.toml/XDG directory.
    std::fs::create_dir_all(&args.data_dir)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(asobi_server::run(args.data_dir, args.listen))
}
