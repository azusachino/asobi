mod commands;
mod dispatch;
mod graph;
mod output;
mod runtime;

use clap::Parser;
use tracing::error;

use commands::Cli;

pub fn run() {
    runtime::init_tracing();
    let cli = Cli::parse();
    let json = cli.json;
    // One command, one single-threaded runtime (ADR 0008). The server (WP4)
    // runs its own multi-threaded runtime; a CLI command has nothing to
    // parallelise, and the current-thread runtime keeps start-up cheap.
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(anyhow::Error::from)
        .and_then(|runtime| runtime.block_on(dispatch::run_cli(cli)));
    if let Err(error) = result {
        if json {
            let error_json = serde_json::json!({
                "status": "failed",
                "error": error.to_string()
            });
            println!("{}", serde_json::to_string_pretty(&error_json).unwrap());
        } else {
            error!("{error:?}");
        }
        std::process::exit(1);
    }
}
