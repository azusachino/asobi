//! Workspace remote configuration and environment overrides.

use asobi_core::paths::AsobiPaths;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfig {
    pub remote: Option<String>,
    pub graph: String,
}

/// Avoid disclosing URL userinfo, query parameters or fragments in CLI output.
pub fn display_endpoint(remote: &str) -> &str {
    if remote.contains(['@', '?', '#']) {
        "<redacted>"
    } else {
        remote
    }
}

/// Resolve config from the discovered workspace file, then environment.
pub fn resolve(paths: &AsobiPaths) -> RemoteConfig {
    resolve_values(
        paths.remote.clone(),
        paths.graph.clone(),
        std::env::var("ASOBI_REMOTE").ok(),
        std::env::var("ASOBI_GRAPH").ok(),
    )
}

fn resolve_values(
    file_remote: Option<String>,
    file_graph: Option<String>,
    env_remote: Option<String>,
    env_graph: Option<String>,
) -> RemoteConfig {
    RemoteConfig {
        remote: env_remote.or(file_remote),
        graph: env_graph
            .or(file_graph)
            .unwrap_or_else(|| "asobi".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_only_config_and_graph_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("asobi.toml"), "remote = 'http://server'\n").unwrap();
        let paths = AsobiPaths::resolve_from(dir.path());
        assert_eq!(paths.remote.as_deref(), Some("http://server"));
        assert_eq!(
            resolve_values(paths.remote, paths.graph, None, None),
            RemoteConfig {
                remote: Some("http://server".into()),
                graph: "asobi".into()
            }
        );
    }

    #[test]
    fn env_only_config() {
        assert_eq!(
            resolve_values(None, None, Some("http://env".into()), Some("other".into())),
            RemoteConfig {
                remote: Some("http://env".into()),
                graph: "other".into()
            }
        );
    }

    #[test]
    fn environment_overrides_file_values() {
        assert_eq!(
            resolve_values(
                Some("http://file".into()),
                Some("file-graph".into()),
                Some("http://env".into()),
                Some("env-graph".into())
            ),
            RemoteConfig {
                remote: Some("http://env".into()),
                graph: "env-graph".into()
            }
        );
    }

    #[test]
    fn no_remote_selects_local_mode() {
        assert_eq!(resolve_values(None, None, None, None).remote, None);
    }
}
