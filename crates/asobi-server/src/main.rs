//! `asobi-server` — the Asobi graph server: named graphs under the data
//! directory, served over HTTP on the tailnet (ADR 0005).

use std::net::SocketAddr;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut listen: Option<String> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "asobi-server {} -- serve named Asobi graphs over HTTP\n\nUSAGE:\n  asobi-server --listen <addr:port>\n\nThe data directory resolves like the CLI's (asobi.toml / XDG).",
                    env!("CARGO_PKG_VERSION")
                );
                return Ok(());
            }
            "--listen" => {
                listen = Some(args.next().expect("--listen requires an addr:port"));
            }
            other => anyhow::bail!(
                "unknown argument {other:?}; usage: asobi-server --listen <addr:port>"
            ),
        }
    }
    let listen = match listen {
        Some(listen) => listen,
        None => anyhow::bail!("usage: asobi-server --listen <addr:port>"),
    };
    let addr: SocketAddr = listen.parse()?;

    // The server's own config and environment drive the sweeps: the data
    // directory (and retention/abandon windows) resolve like the CLI's.
    let data_dir = asobi_core::paths::AsobiPaths::resolve().data_dir;

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(asobi_server::run(data_dir, addr))
}
