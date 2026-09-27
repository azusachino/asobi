//! Command-line parsing for `asobi-server`.
//!
//! Both arguments are required and there is no config-file or XDG fallback
//! for the data directory: a server and a local CLI on one host must never
//! end up on the same graph file, and the hourly sweep must never walk the
//! CLI's directory (WP4 review).

use std::net::SocketAddr;
use std::path::PathBuf;

/// The parsed command line: where to listen, and the server's own data
/// directory (created on startup if missing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub listen: SocketAddr,
    pub data_dir: PathBuf,
}

/// Parse the process arguments (excluding the program name).
pub fn parse_args<I>(args: I) -> anyhow::Result<Args>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut listen: Option<SocketAddr> = None;
    let mut data_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "asobi-server {} -- serve named Asobi graphs over HTTP\n\nUSAGE:\n  asobi-server --listen <addr:port> --data-dir <path>\n\nBoth arguments are required. The data directory is the server's own: it is\ncreated if missing and is never shared with a local CLI workspace.",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "--listen" => {
                let value = args.next().expect("--listen requires an addr:port");
                listen = Some(value.parse()?);
            }
            "--data-dir" => {
                let value = args.next().expect("--data-dir requires a path");
                data_dir = Some(PathBuf::from(value));
            }
            other => anyhow::bail!(
                "unknown argument {other:?}; usage: asobi-server --listen <addr:port> --data-dir <path>"
            ),
        }
    }
    Ok(Args {
        listen: listen.ok_or_else(|| {
            anyhow::anyhow!("missing --listen <addr:port>; usage: asobi-server --listen <addr:port> --data-dir <path>")
        })?,
        data_dir: data_dir.ok_or_else(|| {
            anyhow::anyhow!(
                "missing --data-dir <path>; the server needs its own data directory and must not share the CLI's (asobi.toml/XDG is not read)"
            )
        })?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> anyhow::Result<Args> {
        parse_args(list.iter().map(|s| s.to_string()))
    }

    #[test]
    fn missing_data_dir_is_a_clear_usage_error() {
        let error = args(&["--listen", "127.0.0.1:8300"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--data-dir"), "{error}");
        assert!(
            error.contains("must not share the CLI's"),
            "the error must say why: {error}"
        );
    }

    #[test]
    fn missing_listen_is_a_clear_usage_error() {
        let error = args(&["--data-dir", "/tmp/server-data"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("--listen"), "{error}");
    }

    #[test]
    fn listen_and_data_dir_are_taken_from_the_arguments() {
        let parsed = args(&["--listen", "127.0.0.1:8300", "--data-dir", "/srv/asobi"]).unwrap();
        assert_eq!(parsed.listen, "127.0.0.1:8300".parse().unwrap());
        assert_eq!(parsed.data_dir, PathBuf::from("/srv/asobi"));
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        assert!(
            args(&[
                "--listen",
                "127.0.0.1:8300",
                "--data-dir",
                "/srv",
                "--graph",
                "x"
            ])
            .is_err()
        );
    }
}
