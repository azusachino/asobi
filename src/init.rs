use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Where to set up the workspace.
#[derive(Debug, Clone, Copy)]
pub enum InitTarget {
    /// XDG: a single `$XDG_DATA_HOME/rosemary/` root (default
    /// `~/.local/share/rosemary/`). Default for globally installed users — no
    /// project-local files written.
    Xdg,
    /// Project-local: `<cwd>/.rosemary/{data,topics,config}/` + `rosemary.toml`.
    Local,
}

pub struct InitReport {
    pub target: InitTarget,
    pub created_dirs: Vec<PathBuf>,
    pub skipped_dirs: Vec<PathBuf>,
    pub wrote_config: Option<PathBuf>,
    pub config_existed: Option<PathBuf>,
}

/// Initialise a Rosemary workspace.
///
/// `InitTarget::Xdg` creates the user-level `{data,config,topics}` directories
/// under a single `$XDG_DATA_HOME/rosemary/` root (default
/// `~/.local/share/rosemary/`, honoring `XDG_DATA_HOME` on every platform),
/// all owned by the invoking user.
///
/// `InitTarget::Local` writes a `rosemary.toml` and `.rosemary/` tree into
/// `cwd`. Re-runs are idempotent in both modes.
pub fn init_workspace(target: InitTarget, cwd: &Path) -> Result<InitReport> {
    match target {
        InitTarget::Xdg => init_xdg(),
        InitTarget::Local => init_local(cwd),
    }
}

fn init_xdg() -> Result<InitReport> {
    let xdg = crate::paths::xdg_dirs().ok_or_else(|| {
        anyhow::anyhow!("XDG paths unavailable ($HOME unset); retry with `--local`")
    })?;
    let dirs = [xdg.data_dir, xdg.topics_dir, xdg.config_dir];
    let (created, skipped) = ensure_dirs(&dirs)?;
    Ok(InitReport {
        target: InitTarget::Xdg,
        created_dirs: created,
        skipped_dirs: skipped,
        wrote_config: None,
        config_existed: None,
    })
}

fn init_local(cwd: &Path) -> Result<InitReport> {
    let base = cwd.join(".rosemary");
    let dirs = [base.join("data"), base.join("topics"), base.join("config")];
    let (created, skipped) = ensure_dirs(&dirs)?;

    let config_path = cwd.join("rosemary.toml");
    let (wrote, existed) = if config_path.exists() {
        (None, Some(config_path))
    } else {
        fs::write(&config_path, DEFAULT_LOCAL_CONFIG)
            .with_context(|| format!("write {}", config_path.display()))?;
        (Some(config_path), None)
    };

    Ok(InitReport {
        target: InitTarget::Local,
        created_dirs: created,
        skipped_dirs: skipped,
        wrote_config: wrote,
        config_existed: existed,
    })
}

fn ensure_dirs(dirs: &[PathBuf]) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    for dir in dirs {
        if dir.exists() {
            skipped.push(dir.clone());
        } else {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
            created.push(dir.clone());
        }
    }
    Ok((created, skipped))
}

const DEFAULT_LOCAL_CONFIG: &str = "\
# Rosemary project-local configuration.
# Paths are resolved relative to this file.

data_dir   = \".rosemary/data\"
config_dir = \".rosemary/config\"
topics_dir = \".rosemary/topics\"
";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn local_creates_dirs_and_config_first_time() {
        let dir = tempdir().unwrap();
        let report = init_workspace(InitTarget::Local, dir.path()).unwrap();

        assert!(report.wrote_config.is_some());
        assert_eq!(report.created_dirs.len(), 3);
        assert!(report.skipped_dirs.is_empty());
        assert!(dir.path().join("rosemary.toml").exists());
        assert!(dir.path().join(".rosemary/data").is_dir());
        assert!(dir.path().join(".rosemary/topics").is_dir());
        assert!(dir.path().join(".rosemary/config").is_dir());
    }

    #[test]
    fn local_idempotent_second_run() {
        let dir = tempdir().unwrap();
        init_workspace(InitTarget::Local, dir.path()).unwrap();
        let report = init_workspace(InitTarget::Local, dir.path()).unwrap();

        assert!(report.wrote_config.is_none());
        assert!(report.config_existed.is_some());
        assert!(report.created_dirs.is_empty());
        assert_eq!(report.skipped_dirs.len(), 3);
    }

    #[test]
    fn xdg_creates_unified_root_under_xdg_data_home() {
        let dir = tempdir().unwrap();
        let data = dir.path().join("xdg-data");

        let saved = std::env::var("XDG_DATA_HOME").ok();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &data);
        }

        let report = init_workspace(InitTarget::Xdg, dir.path()).unwrap();

        match saved {
            Some(val) => unsafe { std::env::set_var("XDG_DATA_HOME", val) },
            None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
        }

        assert!(matches!(report.target, InitTarget::Xdg));
        let root = data.join("rosemary");
        assert!(root.join("data").is_dir());
        assert!(root.join("topics").is_dir());
        assert!(root.join("config").is_dir());
    }

    #[test]
    fn local_preserves_existing_config() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("rosemary.toml");
        fs::write(&cfg, "# user-customised\n").unwrap();

        init_workspace(InitTarget::Local, dir.path()).unwrap();
        assert_eq!(fs::read_to_string(&cfg).unwrap(), "# user-customised\n");
    }
}
