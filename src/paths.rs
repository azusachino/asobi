use serde::Deserialize;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct MikuConfig {
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub topics_dir: Option<PathBuf>,
    pub observation_limit: Option<usize>,
}

pub struct MikuPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub topics_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub observation_limit: Option<usize>,
}

pub const ENV_MIKU_HOME: &str = "MIKU_HOME";

/// XDG base directories for the user-level Miku workspace. A single root
/// (`$XDG_DATA_HOME/miku`, honoring the env var on every platform — macOS
/// included, where the `directories` crate would prefer `~/Library/...`) holds
/// the same `{data,config,topics,caches}` subtree as a project-local
/// `.miku/`, so the two layouts mirror each other.
pub struct XdgDirs {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub topics_dir: PathBuf,
    pub cache_dir: PathBuf,
}

/// Resolve `$XDG_DATA_HOME` (or its conventional `~/.local/share` fallback) on
/// any platform. Returns `None` only when both the env var and `$HOME` are
/// unset (e.g. a stripped CI environment).
fn xdg_data_home() -> Option<PathBuf> {
    if let Ok(val) = env::var("XDG_DATA_HOME")
        && !val.trim().is_empty()
    {
        return Some(PathBuf::from(val.trim()));
    }
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".local/share"))
}

/// XDG paths for the user-level Miku workspace, rooted at a single
/// `$XDG_DATA_HOME/miku/` directory. `None` when `$XDG_DATA_HOME` and
/// `$HOME` are both unset.
pub fn xdg_dirs() -> Option<XdgDirs> {
    let root = xdg_data_home()?.join("miku");
    Some(XdgDirs {
        data_dir: root.join("data"),
        config_dir: root.join("config"),
        topics_dir: root.join("topics"),
        cache_dir: root.join("caches"),
    })
}

impl MikuPaths {
    pub fn resolve() -> Self {
        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::resolve_from(&cwd)
    }

    /// Resolution order:
    /// 1. `MIKU_HOME` env var (forces a unified root, bypasses discovery).
    /// 2. Nearest `miku.toml` walking up from `start` — relative paths in
    ///    the config are anchored to the config file's directory, not cwd.
    /// 3. Nearest `.miku/` directory walking up from `start`.
    /// 4. XDG fallback.
    pub fn resolve_from(start: &Path) -> Self {
        if let Ok(home) = env::var(ENV_MIKU_HOME) {
            let root = PathBuf::from(home);
            return Self {
                cache_dir: root.join("caches"),
                data_dir: root.clone(),
                config_dir: root.clone(),
                topics_dir: root,
                observation_limit: None,
            };
        }

        if let Some(config_path) = find_upwards(start, "miku.toml", false)
            && let Ok(content) = std::fs::read_to_string(&config_path)
            && let Ok(conf) = toml::from_str::<MikuConfig>(&content)
        {
            let anchor = config_path.parent().unwrap_or(Path::new("."));
            return Self::from_config(conf, anchor);
        }

        if let Some(local_root) = find_upwards(start, ".miku", true) {
            return Self {
                data_dir: local_root.join("data"),
                config_dir: local_root.join("config"),
                topics_dir: local_root.join("topics"),
                cache_dir: local_root.join("caches"),
                observation_limit: None,
            };
        }

        match xdg_dirs() {
            Some(x) => Self {
                data_dir: x.data_dir,
                config_dir: x.config_dir,
                topics_dir: x.topics_dir,
                cache_dir: x.cache_dir,
                observation_limit: None,
            },
            None => Self {
                data_dir: PathBuf::from(".miku/data"),
                config_dir: PathBuf::from(".miku/config"),
                topics_dir: PathBuf::from(".miku/topics"),
                cache_dir: PathBuf::from(".miku/caches"),
                observation_limit: None,
            },
        }
    }

    fn from_config(conf: MikuConfig, anchor: &Path) -> Self {
        let resolve = |p: Option<PathBuf>, default: &str| -> PathBuf {
            let raw = p.unwrap_or_else(|| PathBuf::from(default));
            if raw.is_absolute() {
                raw
            } else {
                anchor.join(raw)
            }
        };
        let data_dir = resolve(conf.data_dir, ".miku/data");
        let cache_dir = data_dir
            .parent()
            .map(|p| p.join("caches"))
            .unwrap_or_else(|| PathBuf::from(".miku/caches"));
        Self {
            config_dir: resolve(conf.config_dir, ".miku/config"),
            topics_dir: resolve(conf.topics_dir, ".miku/topics"),
            data_dir,
            cache_dir,
            observation_limit: conf.observation_limit,
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("miku.db")
    }

    pub fn caches_dir(&self) -> PathBuf {
        self.cache_dir.clone()
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
        let cfg = dir.path().join("miku.toml");
        std::fs::write(
            &cfg,
            r#"
            data_dir = "custom-data"
            "#,
        )
        .unwrap();

        let paths = MikuPaths::resolve_from(dir.path());
        assert_eq!(paths.data_dir, dir.path().join("custom-data"));
    }

    #[test]
    fn absolute_paths_in_toml_preserved() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("miku.toml");
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

        let paths = MikuPaths::resolve_from(dir.path());
        assert_eq!(paths.data_dir, PathBuf::from(abs_path));
    }

    #[test]
    fn dot_miku_dir_discovered_walking_up() {
        let dir = tempdir().unwrap();
        let local_root = dir.path().join(".miku");
        std::fs::create_dir_all(local_root.join("data")).unwrap();

        let sub = dir.path().join("a/b/c");
        std::fs::create_dir_all(&sub).unwrap();

        let paths = MikuPaths::resolve_from(&sub);
        assert_eq!(paths.data_dir, local_root.join("data"));
    }

    #[test]
    fn miku_home_overrides_discovery() {
        let dir = tempdir().unwrap();
        let home = dir.path().join("fake-home");
        std::fs::create_dir_all(&home).unwrap();

        let prev = env::var(ENV_MIKU_HOME).ok();
        unsafe { env::set_var(ENV_MIKU_HOME, &home) };

        let paths = MikuPaths::resolve();

        match prev {
            Some(v) => unsafe { env::set_var(ENV_MIKU_HOME, v) },
            None => unsafe { env::remove_var(ENV_MIKU_HOME) },
        }

        assert_eq!(paths.data_dir, home);
    }

    #[test]
    fn xdg_data_home_drives_unified_root_on_all_platforms() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("xdg-data");

        let saved: Vec<(&str, Option<String>)> = [ENV_MIKU_HOME, "XDG_DATA_HOME"]
            .iter()
            .map(|k| (*k, env::var(*k).ok()))
            .collect();

        unsafe {
            env::remove_var(ENV_MIKU_HOME);
            env::set_var("XDG_DATA_HOME", &data);
        }

        // Resolve from a dir with no miku.toml / .miku upward.
        let probe = dir.path().join("probe");
        std::fs::create_dir_all(&probe).unwrap();
        let paths = MikuPaths::resolve_from(&probe);

        for (k, v) in saved {
            match v {
                Some(val) => unsafe { env::set_var(k, val) },
                None => unsafe { env::remove_var(k) },
            }
        }

        // Single root mirrors the project-local `.miku/{...}` layout.
        let root = data.join("miku");
        assert_eq!(paths.data_dir, root.join("data"));
        assert_eq!(paths.config_dir, root.join("config"));
        assert_eq!(paths.topics_dir, root.join("topics"));
        assert_eq!(paths.caches_dir(), root.join("caches"));
    }
}
