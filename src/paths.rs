use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct RosemaryConfig {
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub topics_dir: Option<PathBuf>,
}

pub struct RosemaryPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub topics_dir: PathBuf,
}

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
        if let Ok(home) = env::var("ROSEMARY_HOME") {
            let root = PathBuf::from(home);
            return Self {
                data_dir: root.clone(),
                config_dir: root.clone(),
                topics_dir: root,
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
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("rosemary.db")
    }
}

/// Walk up from `start` looking for a file (or directory if `is_dir`) named `name`.
/// Returns the full path to the match, or `None` if not found before the root.
fn find_upwards(start: &Path, name: &str, is_dir: bool) -> Option<PathBuf> {
    let mut cur = start;
    loop {
        let candidate = cur.join(name);
        let matches = if is_dir {
            candidate.is_dir()
        } else {
            candidate.is_file()
        };
        if matches {
            return Some(candidate);
        }
        match cur.parent() {
            Some(parent) => cur = parent,
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    // Env vars are process-global; serialize tests that touch them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_clean_env<T>(f: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = env::var("ROSEMARY_HOME").ok();
        // SAFETY: tests serialize on ENV_LOCK.
        unsafe { env::remove_var("ROSEMARY_HOME") };
        let result = f();
        unsafe {
            match prev {
                Some(v) => env::set_var("ROSEMARY_HOME", v),
                None => env::remove_var("ROSEMARY_HOME"),
            }
        }
        result
    }

    #[test]
    fn toml_relative_paths_anchor_to_config_dir_not_cwd() {
        with_clean_env(|| {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::write(
                root.join("rosemary.toml"),
                "data_dir = \".rosemary/data\"\n\
                 config_dir = \".rosemary/config\"\n\
                 topics_dir = \".rosemary/topics\"\n",
            )
            .unwrap();

            let subdir = root.join("nested/deep");
            std::fs::create_dir_all(&subdir).unwrap();

            let paths = RosemaryPaths::resolve_from(&subdir);
            assert_eq!(paths.data_dir, root.join(".rosemary/data"));
            assert_eq!(paths.topics_dir, root.join(".rosemary/topics"));
        });
    }

    #[test]
    fn absolute_paths_in_toml_preserved() {
        with_clean_env(|| {
            let dir = tempdir().unwrap();
            let abs = dir.path().join("absolute_data");
            std::fs::write(
                dir.path().join("rosemary.toml"),
                format!("data_dir = \"{}\"\n", abs.display()),
            )
            .unwrap();

            let paths = RosemaryPaths::resolve_from(dir.path());
            assert_eq!(paths.data_dir, abs);
        });
    }

    #[test]
    fn rosemary_home_overrides_discovery() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let home = dir.path().join("override");
        std::fs::create_dir_all(&home).unwrap();
        // Drop a rosemary.toml that would otherwise win.
        std::fs::write(dir.path().join("rosemary.toml"), "").unwrap();

        let prev = env::var("ROSEMARY_HOME").ok();
        // SAFETY: serialized on ENV_LOCK.
        unsafe { env::set_var("ROSEMARY_HOME", &home) };
        let paths = RosemaryPaths::resolve_from(dir.path());
        unsafe {
            match prev {
                Some(v) => env::set_var("ROSEMARY_HOME", v),
                None => env::remove_var("ROSEMARY_HOME"),
            }
        }
        assert_eq!(paths.data_dir, home);
    }

    #[test]
    fn dot_rosemary_dir_discovered_walking_up() {
        with_clean_env(|| {
            let dir = tempdir().unwrap();
            let root = dir.path();
            std::fs::create_dir_all(root.join(".rosemary")).unwrap();
            let sub = root.join("a/b/c");
            std::fs::create_dir_all(&sub).unwrap();

            let paths = RosemaryPaths::resolve_from(&sub);
            assert_eq!(paths.data_dir, root.join(".rosemary/data"));
        });
    }
}
