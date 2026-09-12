//! The declarative skill configuration: a `[skills]` block in `asobi.toml`.
//!
//! `skills install` is imperative — it does what you type, once. This module
//! backs `skills sync`, where the config file is the whole truth: whatever it
//! declares is installed, and whatever it does not declare is removed.

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where synced skills are written when `[skills].path` is absent.
pub const DEFAULT_SKILLS_PATH: &str = ".agents/skills";

/// The `[skills]` block. Parsed on its own rather than as a field of
/// `paths::AsobiConfig`, so a malformed skill declaration surfaces as a `sync`
/// error instead of silently breaking workspace path resolution.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    /// Directory the selected skills are written to, relative to the
    /// `asobi.toml` that declares it. Defaults to `.agents/skills`.
    pub path: Option<PathBuf>,
    /// The declared sources, in file order.
    #[serde(default, rename = "source")]
    pub sources: Vec<SkillSource>,
}

/// One declared source: a git URL or local path, plus which of its skills to
/// take.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkillSource {
    /// Git URL or local directory path.
    pub url: String,
    /// Take every skill the source offers.
    #[serde(default)]
    pub all: bool,
    /// Take only these skills, by name.
    #[serde(default)]
    pub select: Vec<String>,
    /// Scope the install walk to this subdirectory of the checkout, relative
    /// to its root. Some sources mirror every skill across several
    /// tool-specific directories (`.opencode/`, `.kiro/`, a canonical
    /// `skills/`, ...) with the same `name:` in each copy, which collides on
    /// install; scoping to the one canonical directory avoids the mirrors
    /// entirely instead of asking the install step to arbitrate between them.
    #[serde(default)]
    pub subdir: Option<PathBuf>,
    /// Pin the source to a commit, tag, or branch instead of following its
    /// default branch.
    ///
    /// Without this, a re-sync silently adopts whatever the source has moved to
    /// since — and a skill is natural-language instruction loaded straight into
    /// an agent's context, so that is an unreviewed behaviour change rather than
    /// a dependency bump. The resolved commit is recorded either way; pinning is
    /// what makes adopting a new one a decision.
    #[serde(default)]
    pub rev: Option<String>,
    /// Explicit checkout-relative Markdown files shared by selected skills.
    #[serde(default)]
    pub shared_markdown: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct SkillsBlock {
    skills: Option<SkillsConfig>,
}

impl SkillSource {
    /// The selection this source declares. `all` and `select` are mutually
    /// exclusive, and one of them is required — an unqualified source would
    /// otherwise mean "prompt", which a declarative sync cannot honour.
    pub fn selection(&self) -> Result<crate::skills::SelectionMode> {
        for path in &self.shared_markdown {
            crate::skill_resources::validate_resource_path(path)?;
        }
        match (self.all, self.select.is_empty()) {
            (true, false) => bail!(
                "source '{}' declares both `all` and `select`; pick one",
                self.url
            ),
            (true, true) => Ok(crate::skills::SelectionMode::All),
            (false, false) => Ok(crate::skills::SelectionMode::Select(self.select.clone())),
            (false, true) => bail!(
                "source '{}' declares neither `all = true` nor `select = [...]`",
                self.url
            ),
        }
    }
}

impl SkillsConfig {
    /// Read the `[skills]` block from `config_file`. Returns `None` when the
    /// file declares no skills at all.
    pub fn load(config_file: &Path) -> Result<Option<Self>> {
        let content = std::fs::read_to_string(config_file)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", config_file.display()))?;
        let block: SkillsBlock = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("parse {}: {e}", config_file.display()))?;
        Ok(block.skills)
    }

    /// The directory selected skills are written to, anchored to `root` when
    /// the declared path is relative.
    pub fn resolved_path(&self, root: &Path) -> PathBuf {
        let raw = self
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(DEFAULT_SKILLS_PATH));
        if raw.is_absolute() {
            raw
        } else {
            root.join(raw)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::SelectionMode;
    use tempfile::tempdir;

    fn write_config(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("asobi.toml");
        std::fs::write(&path, body).unwrap();
        (dir, path)
    }

    #[test]
    fn loads_sources_in_file_order() {
        let (_dir, cfg) = write_config(
            r#"
data_dir = ".asobi/data"

[[skills.source]]
url = "https://github.com/a/one"
all = true

[[skills.source]]
url = "https://github.com/b/two"
select = ["alpha", "beta"]
"#,
        );

        let skills = SkillsConfig::load(&cfg).unwrap().unwrap();
        assert_eq!(skills.sources.len(), 2);
        assert_eq!(skills.sources[0].url, "https://github.com/a/one");
        assert_eq!(skills.sources[0].selection().unwrap(), SelectionMode::All);
        assert_eq!(
            skills.sources[1].selection().unwrap(),
            SelectionMode::Select(vec!["alpha".into(), "beta".into()])
        );
    }

    #[test]
    fn subdir_defaults_to_none_and_parses_when_declared() {
        let (_dir, cfg) = write_config(
            r#"
[[skills.source]]
url = "https://github.com/a/one"
all = true

[[skills.source]]
url = "https://github.com/b/two"
select = ["alpha"]
subdir = "skills"
"#,
        );

        let skills = SkillsConfig::load(&cfg).unwrap().unwrap();
        assert_eq!(skills.sources[0].subdir, None);
        assert_eq!(skills.sources[1].subdir, Some(PathBuf::from("skills")));
    }

    #[test]
    fn no_skills_block_is_none_not_an_error() {
        let (_dir, cfg) = write_config("data_dir = \".asobi/data\"\n");
        assert!(SkillsConfig::load(&cfg).unwrap().is_none());
    }

    #[test]
    fn selection_requires_exactly_one_of_all_or_select() {
        let both = SkillSource {
            url: "u".into(),
            all: true,
            select: vec!["a".into()],
            subdir: None,
            rev: None,
            shared_markdown: vec![],
        };
        assert!(both.selection().is_err());

        let neither = SkillSource {
            url: "u".into(),
            all: false,
            select: vec![],
            subdir: None,
            rev: None,
            shared_markdown: vec![],
        };
        assert!(neither.selection().is_err());
    }

    #[test]
    fn path_defaults_to_agents_skills_anchored_at_root() {
        let cfg = SkillsConfig::default();
        assert_eq!(
            cfg.resolved_path(Path::new("/proj")),
            Path::new("/proj/.agents/skills")
        );
    }

    #[test]
    fn absolute_path_is_not_anchored() {
        let cfg = SkillsConfig {
            path: Some(PathBuf::from("/elsewhere/skills")),
            sources: vec![],
        };
        assert_eq!(
            cfg.resolved_path(Path::new("/proj")),
            Path::new("/elsewhere/skills")
        );
    }

    #[test]
    fn unknown_key_in_a_source_is_rejected() {
        let (_dir, cfg) = write_config(
            r#"
[[skills.source]]
url = "https://github.com/a/one"
alll = true
"#,
        );
        assert!(SkillsConfig::load(&cfg).is_err());
    }
}
