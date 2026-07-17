use super::commands::Commands;
#[cfg(feature = "documents")]
use crate::paths::AsobiPaths;
use anyhow::Result;
#[cfg(feature = "documents")]
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;

#[derive(Debug, Clone, Copy, Default)]
struct LocalTimer;

impl FormatTime for LocalTimer {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
    }
}

#[cfg(feature = "documents")]
pub(crate) fn needs_vector(cmd: &Commands) -> bool {
    matches!(
        cmd,
        Commands::Ingest { .. } | Commands::Query { .. } | Commands::Compact { .. }
    )
}

#[cfg(not(feature = "documents"))]
pub(crate) fn needs_vector(_: &Commands) -> bool {
    false
}

#[cfg(feature = "documents")]
pub const ENV_FASTEMBED_CACHE_DIR: &str = "ASOBI_FASTEMBED_CACHE_DIR";
#[cfg(feature = "documents")]
pub const ENV_TOPICS_DIR: &str = "ASOBI_TOPICS_DIR";

#[cfg(feature = "documents")]
pub(crate) fn init_embedder(paths: &AsobiPaths) -> Result<Arc<crate::embed::FastEmbedProvider>> {
    let cache_dir = std::env::var(ENV_FASTEMBED_CACHE_DIR)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| paths.data_dir.join("fastembed_cache"));
    let embedder: Arc<crate::embed::FastEmbedProvider> =
        Arc::new(crate::embed::FastEmbedProvider::new(cache_dir)?);
    Ok(embedder)
}

/// Initialise the global tracing subscriber. Logs go to **stderr** so the
/// stdout channel stays clean for machine-readable data (graph JSON, stats) and
/// the MCP JSON-RPC stream. Level is controlled by `RUST_LOG` (default `info`).
pub(crate) fn init_tracing() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .with_timer(LocalTimer)
        .compact()
        .init();
}

/// Verify the `git` binary is reachable before any remote operation, so a
/// missing git yields a clear message instead of a raw `os error 2` from `?`.
pub(crate) fn ensure_git_available() -> Result<()> {
    match std::process::Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => anyhow::bail!("`git --version` failed; ensure git is installed and on PATH"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => anyhow::bail!(
            "`git` not found on PATH — install git to install or update skills from a remote repository"
        ),
        Err(e) => anyhow::bail!("failed to invoke git: {e}"),
    }
}

pub(crate) fn validate_git_url(git_url: &str) -> Result<()> {
    if git_url.starts_with('-') {
        anyhow::bail!("Invalid git URL: URLs must not start with '-'");
    }

    let has_allowed_scheme = ["https://", "ssh://", "git://", "file://"]
        .iter()
        .any(|scheme| git_url.starts_with(scheme));
    let is_scp_style = git_url.starts_with("git@") && git_url.contains(':');
    if !has_allowed_scheme && !is_scp_style {
        anyhow::bail!(
            "Unsupported git URL '{}'; use https://, ssh://, git://, file://, or git@host:path",
            git_url
        );
    }

    Ok(())
}

pub(crate) fn get_or_update_cached_repo(
    git_url: &str,
    caches_dir: &std::path::Path,
) -> Result<(std::path::PathBuf, String)> {
    ensure_git_available()?;
    validate_git_url(git_url)?;
    let slug = crate::skills::derive_source_slug(git_url);
    let repo_cache_dir = caches_dir.join(&slug);

    std::fs::create_dir_all(caches_dir)?;

    if repo_cache_dir.exists() {
        info!("Updating cached repository in {:?}...", repo_cache_dir);
        let fetch_status = std::process::Command::new("git")
            .arg("fetch")
            .arg("--depth")
            .arg("1")
            .current_dir(&repo_cache_dir)
            .status();

        let mut success = false;
        if let Ok(status) = fetch_status
            && status.success()
        {
            let reset_status = std::process::Command::new("git")
                .arg("reset")
                .arg("--hard")
                .arg("origin/HEAD")
                .current_dir(&repo_cache_dir)
                .status();
            if let Ok(status) = reset_status
                && status.success()
            {
                success = true;
            }
        }

        if !success {
            info!(
                "Failed to update existing cache, re-cloning to {:?}...",
                repo_cache_dir
            );
            let _ = std::fs::remove_dir_all(&repo_cache_dir);
            let clone_status = std::process::Command::new("git")
                .arg("clone")
                .arg("--depth")
                .arg("1")
                .arg("--")
                .arg(git_url)
                .arg(&repo_cache_dir)
                .status()?;
            if !clone_status.success() {
                anyhow::bail!("Failed to clone repository from {}", git_url);
            }
        }
    } else {
        info!("Cloning {} to {:?}...", git_url, repo_cache_dir);
        let clone_status = std::process::Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--")
            .arg(git_url)
            .arg(&repo_cache_dir)
            .status()?;
        if !clone_status.success() {
            anyhow::bail!("Failed to clone repository from {}", git_url);
        }
    }

    let output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .current_dir(&repo_cache_dir)
        .output()?;
    let version = if output.status.success() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        "unknown".to_string()
    };

    Ok((repo_cache_dir, version))
}
