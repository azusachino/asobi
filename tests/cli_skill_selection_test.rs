use asobi::skills::{SelectionMode, SkillsTree, collect_skills_from_dir, materialize_skills};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("source/example");
        std::fs::create_dir_all(&source).unwrap();
        Self {
            _temp: temp,
            root,
            source,
        }
    }

    fn skill(&self, path: &str, name: &str, body: &str) {
        write(
            &self.source.join(path).join("SKILL.md"),
            &format!("---\nname: {name}\ndescription: selection fixture\n---\n{body}\n"),
        );
    }

    fn config(&self, select: &[&str]) {
        write(
            &self.root.join("asobi.toml"),
            &format!(
                "data_dir = \"state\"\n[[skills.source]]\nurl = {:?}\nselect = {select:?}\n",
                self.source.to_str().unwrap()
            ),
        );
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_asobi"))
            .current_dir(&self.root)
            .env_remove("ASOBI_HOME")
            .env("ASOBI_DATABASE_URL", self.root.join("state/asobi.db"))
            .args(args)
            .output()
            .unwrap()
    }

    fn success(&self, args: &[&str]) {
        let result = self.run(args);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn installed(&self, name: &str) -> PathBuf {
        self.root
            .join(".agents/skills")
            .join(format!("source-example@{name}/SKILL.md"))
    }
}

#[test]
fn paths_unique_suffixes_and_names_select_the_same_skill_once() {
    let f = Fixture::new();
    f.skill(
        "skills/testing/test-driven-development",
        "Test-Driven Development (TDD)",
        "Selected body.",
    );
    f.config(&[
        "skills/testing/test-driven-development",
        "testing/test-driven-development",
        "test-driven-development",
        "Test-Driven Development (TDD)",
    ]);
    f.success(&["skills", "sync"]);
    assert!(f.installed("test-driven-development-tdd").exists());
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.root.join("state/skills.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["skills"].as_array().unwrap().len(), 1);
}

#[test]
fn ambiguous_suffixes_and_display_names_report_matching_paths() {
    for selector in ["review", "Shared display name"] {
        let f = Fixture::new();
        f.skill("skills/frontend/review", "Shared display name", "Frontend.");
        f.skill("skills/backend/review", "Shared display name", "Backend.");
        f.config(&[selector]);
        let result = f.run(&["skills", "sync"]);
        assert!(!result.status.success());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("ambiguous"), "{stderr}");
        assert!(stderr.contains("skills/frontend/review"));
        assert!(stderr.contains("skills/backend/review"));
        assert!(!f.root.join(".agents/skills").exists());
    }
}

#[test]
fn exact_path_selects_one_of_duplicate_names() {
    let f = Fixture::new();
    f.skill("skills/frontend/review", "Review", "Frontend body.");
    f.skill("skills/backend/review", "Review", "Backend body.");
    f.config(&["skills/frontend/review"]);
    f.success(&["skills", "sync"]);
    assert!(
        std::fs::read_to_string(f.installed("review"))
            .unwrap()
            .contains("Frontend body.")
    );
}

#[test]
fn normalized_directory_collision_fails_before_installation() {
    let f = Fixture::new();
    f.skill("one", "A B", "First.");
    f.skill("two", "A-B", "Second.");
    f.config(&["A B", "A-B"]);
    let result = f.run(&["skills", "sync"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("collision"));
    assert!(!f.root.join(".agents/skills").exists());
}

#[test]
fn imperative_path_selector_survives_display_name_changes_on_update() {
    let f = Fixture::new();
    f.skill("skills/testing/practice", "Old display", "Version one.");
    f.config(&["Old display"]);
    f.success(&[
        "skills",
        "install",
        f.source.to_str().unwrap(),
        "--subdir",
        "skills",
        "--select",
        "testing/practice",
    ]);
    f.skill("skills/testing/practice", "New display", "Version two.");
    f.success(&["skills", "update"]);
    assert!(
        std::fs::read_to_string(f.installed("new-display"))
            .unwrap()
            .contains("Version two.")
    );
    assert!(!f.installed("old-display").exists());
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.root.join("state/skills.json")).unwrap())
            .unwrap();
    assert_eq!(
        manifest["skills"][0]["sourceConfig"]["select"][0],
        "testing/practice"
    );
}

#[test]
fn materialization_rejects_duplicate_destinations_from_any_caller() {
    let f = Fixture::new();
    f.skill("alpha", "alpha", "Original.");
    let mut desired =
        collect_skills_from_dir(&f.source, "source", "local", SelectionMode::All, false).unwrap();
    let mut collision = desired[0].clone();
    collision.body = "Overwritten.".into();
    desired.push(collision);
    let target = f.root.join("installed");
    let result = materialize_skills(&SkillsTree::new(&f.root.join("state"), &target), &desired);
    assert!(result.is_err());
    assert!(!target.exists());
}
