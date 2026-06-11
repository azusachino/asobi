use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct RosemaryConfig {
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub topics_dir: Option<PathBuf>,
    pub observation_limit: Option<usize>,
}

pub struct RosemaryPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub topics_dir: PathBuf,
    pub observation_limit: Option<usize>,
}

pub const ENV_ROSEMARY_HOME: &str = "ROSEMARY_HOME";

impl RosemaryPaths {
    pub fn resolve() -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::resolve_from(&cwd)
    }

    /// Resolution order:
    /// 1. `ROSEMARY_HOME` env var (forces a unified root, bypasses discovery).
    /// 2. Nearest `rosemary.toml` walking up from `start` — relative paths in
    ///    the config are anchored to the config file's directory, not cwd.
    /// 3. Nearest `.rosemary/` directory walking up from `start`.
    /// 4. XDG fallback.
    pub fn resolve_from(start: &Path) -> Self {
        if let Ok(home) = env::var(ENV_ROSEMARY_HOME) {
            let root = PathBuf::from(home);
            return Self {
                data_dir: root.clone(),
                config_dir: root.clone(),
                topics_dir: root,
                observation_limit: None,
            };
        }

        if let Some(config_path) = find_upwards(start, "rosemary.toml", false)
            && let Ok(content) = std::fs::read_to_string(&config_path)
            && let Ok(conf) = toml::from_str::<RosemaryConfig>(&content)
        {
            let anchor = config_path.parent().unwrap_or(Path::new("."));
            return Self::from_config(conf, anchor);
        }

        if let Some(local_root) = find_upwards(start, ".rosemary", true) {
            return Self {
                data_dir: local_root.join("data"),
                config_dir: local_root.join("config"),
                topics_dir: local_root.join("topics"),
                observation_limit: None,
            };
        }

        let proj_dirs = directories::ProjectDirs::from("me", "azusachino", "rosemary");
        let data_dir = proj_dirs
            .as_ref()
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".rosemary/data"));
        let config_dir = proj_dirs
            .as_ref()
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".rosemary/config"));
        let topics_dir = proj_dirs
            .as_ref()
            .map(|d| d.data_dir().join("topics"))
            .unwrap_or_else(|| PathBuf::from(".rosemary/topics"));

        Self {
            data_dir,
            config_dir,
            topics_dir,
            observation_limit: None,
        }
    }

    fn from_config(conf: RosemaryConfig, anchor: &Path) -> Self {
        let resolve = |p: Option<PathBuf>, default: &str| -> PathBuf {
            let raw = p.unwrap_or_else(|| PathBuf::from(default));
            if raw.is_absolute() {
                raw
            } else {
                anchor.join(raw)
            }
        };
        Self {
            data_dir: resolve(conf.data_dir, ".rosemary/data"),
            config_dir: resolve(conf.config_dir, ".rosemary/config"),
            topics_dir: resolve(conf.topics_dir, ".rosemary/topics"),
            observation_limit: conf.observation_limit,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("rosemary.db")
    }
}

/// Walk up from `start` looking for a file (or directory if `is_dir`) named `name`.
/// Returns the full path to the match, or `None` if not found before the root.
fn find_upwards(start: &Path, name: &str, is_dir: bool) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let target = current.join(name);
        if (is_dir && target.is_dir()) || (!is_dir && target.is_file()) {
            return Some(target);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn toml_relative_paths_anchor_to_config_dir_not_cwd() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("rosemary.toml");
        std::fs::write(
            &cfg,
            r#"
            data_dir = "custom-data"
            "#,
        )
        .unwrap();

        let paths = RosemaryPaths::resolve_from(dir.path());
        assert_eq!(paths.data_dir, dir.path().join("custom-data"));
    }

    #[test]
    fn absolute_paths_in_toml_preserved() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("rosemary.toml");
        let abs_path = if cfg!(windows) {
            "C:\\data"
        } else {
            "/tmp/data"
        };
        std::fs::write(
            &cfg,
            format!(
                r#"
            data_dir = "{}"
            "#,
                abs_path.replace('\\', "\\\\")
            ),
        )
        .unwrap();

        let paths = RosemaryPaths::resolve_from(dir.path());
        assert_eq!(paths.data_dir, PathBuf::from(abs_path));
    }

    #[test]
    fn dot_rosemary_dir_discovered_walking_up() {
        let dir = tempdir().unwrap();
        let local_root = dir.path().join(".rosemary");
        std::fs::create_dir_all(local_root.join("data")).unwrap();

        let sub = dir.path().join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();

        let paths = RosemaryPaths::resolve_from(&sub);
        assert_eq!(paths.data_dir, local_root.join("data"));
    }

    #[test]
    fn rosemary_home_overrides_discovery() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("fake-home");
        std::fs::create_dir_all(&home).unwrap();

        let prev = env::var(ENV_ROSEMARY_HOME).ok();
        unsafe { env::set_var(ENV_ROSEMARY_HOME, &home) };

        let paths = RosemaryPaths::resolve();

        match prev {
            Some(v) => unsafe { env::set_var(ENV_ROSEMARY_HOME, v) },
            None => unsafe { env::remove_var(ENV_ROSEMARY_HOME) },
        }

        assert_eq!(paths.data_dir, home);
    }
}
