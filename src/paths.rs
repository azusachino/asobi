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
        // 1. Project-local rosemary.toml
        let local_config = Path::new("rosemary.toml");
        if local_config.exists()
            && let Ok(content) = std::fs::read_to_string(local_config)
            && let Ok(conf) = toml::from_str::<RosemaryConfig>(&content)
        {
            return Self::from_config(conf, Path::new("."));
        }

        // 2. Project-local .rosemary directory
        let local_root = PathBuf::from(".rosemary");
        if local_root.exists() {
            return Self {
                data_dir: local_root.join("data"),
                config_dir: local_root.join("config"),
                topics_dir: local_root.join("topics"),
            };
        }

        // 3. XDG fallback
        let home = env::var("ROSEMARY_HOME").map(PathBuf::from).ok();
        let proj_dirs = directories::ProjectDirs::from("me", "azusachino", "rosemary");

        let data_dir = home.clone().unwrap_or_else(|| {
            proj_dirs
                .as_ref()
                .map(|d| d.data_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".rosemary/data"))
        });

        let config_dir = home.clone().unwrap_or_else(|| {
            proj_dirs
                .as_ref()
                .map(|d| d.config_dir().to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".rosemary/config"))
        });

        let topics_dir = home.unwrap_or_else(|| {
            proj_dirs
                .as_ref()
                .map(|d| d.data_dir().join("topics"))
                .unwrap_or_else(|| PathBuf::from(".rosemary/topics"))
        });

        Self {
            data_dir,
            config_dir,
            topics_dir,
        }
    }

    fn from_config(conf: RosemaryConfig, root: &Path) -> Self {
        Self {
            data_dir: conf.data_dir.unwrap_or_else(|| root.join(".rosemary/data")),
            config_dir: conf
                .config_dir
                .unwrap_or_else(|| root.join(".rosemary/config")),
            topics_dir: conf
                .topics_dir
                .unwrap_or_else(|| root.join(".rosemary/topics")),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("rosemary.db")
    }
}
