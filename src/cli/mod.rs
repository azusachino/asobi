mod commands;
mod dispatch;
mod graph;
mod output;
mod runtime;
mod skills;

use clap::Parser;
use tracing::error;

use commands::Cli;

pub fn run() {
    runtime::init_tracing();
    let cli = Cli::parse();
    let json = cli.json;
    if let Err(error) = dispatch::run_cli(cli) {
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
