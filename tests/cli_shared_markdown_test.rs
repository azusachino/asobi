use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    source: PathBuf,
}

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let source = root.join("upstream/example");
        write(
            &source.join("skills/alpha/SKILL.md"),
            "---\nname: alpha\ndescription: example\n---\nSee `../../references/check.md#gate` and [guide](../../references/nested/guide.md).\n",
        );
        write(
            &source.join("skills/alpha/notes.md"),
            "Read `../../references/check.md`.\n",
        );
        write(
            &source.join("skills/beta/SKILL.md"),
            "---\nname: beta\ndescription: example\n---\nNot selected.\n",
        );
        write(
            &source.join("references/check.md"),
            "# Gate\nSee [guide](nested/guide.md).\n",
        );
        write(
            &source.join("references/nested/guide.md"),
            "See `../check.md#gate`.\n",
        );
        write(
            &source.join("references/payload.sh"),
            "must never be installed\n",
        );
        let fixture = Self {
            _temp: temp,
            root,
            source,
        };
        fixture.config(true);
        fixture
    }

    fn config(&self, resources: bool) {
        write(
            &self.root.join("asobi.toml"),
            &format!(
                "data_dir = \"state\"\n[[skills.source]]\nurl = {:?}\nsubdir = \"skills\"\nselect = [\"alpha\"]\nshared_markdown = {}\n",
                self.source.to_str().unwrap(),
                if resources {
                    "[\"references/check.md\", \"references/nested/guide.md\"]"
                } else {
                    "[]"
                }
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

    fn installed(&self, rel: &str) -> PathBuf {
        self.root.join(".agents/skills").join(rel)
    }
}

#[test]
fn shared_markdown_lifecycle_preserves_source_policy_and_rewrites_links() {
    let f = Fixture::new();
    write(
        &f.source.join("mirror/alpha/SKILL.md"),
        "---\nname: alpha\n---\nUnselected mirror.\n",
    );
    f.success(&["skills", "sync"]);
    let skill = f.installed("upstream-example@alpha/SKILL.md");
    let resource = f.installed(".shared/upstream-example/references/check.md");
    let body = std::fs::read_to_string(&skill).unwrap();
    assert!(body.contains("`../.shared/upstream-example/references/check.md#gate`"));
    assert!(body.contains("](../.shared/upstream-example/references/nested/guide.md)"));
    assert!(
        std::fs::read_to_string(f.installed("upstream-example@alpha/notes.md"))
            .unwrap()
            .contains("../.shared/upstream-example/references/check.md")
    );
    assert!(
        std::fs::read_to_string(f.installed(".shared/upstream-example/references/nested/guide.md"))
            .unwrap()
            .contains("`../check.md#gate`")
    );
    assert!(
        !f.installed(".shared/upstream-example/references/payload.sh")
            .exists()
    );
    assert!(!f.installed("upstream-example@beta").exists());
    let mtime = std::fs::metadata(&resource).unwrap().modified().unwrap();
    f.success(&["skills", "sync"]);
    assert_eq!(
        mtime,
        std::fs::metadata(&resource).unwrap().modified().unwrap()
    );
    write(
        &f.source.join("references/check.md"),
        "Updated shared text.\n",
    );
    f.success(&["skills", "update"]);
    assert_eq!(
        std::fs::read_to_string(&resource).unwrap(),
        "Updated shared text.\n"
    );
    assert!(!f.installed("upstream-example@beta").exists());
    let manifest = std::fs::read_to_string(f.root.join("state/skills.json")).unwrap();
    assert!(manifest.contains("references/check.md"));
    assert!(manifest.contains("sourceConfig"));
    f.success(&["skills", "remove", "alpha"]);
    assert!(!resource.exists());
    assert!(!skill.exists());
}

#[test]
fn config_omission_prunes_only_owned_resources() {
    let f = Fixture::new();
    f.success(&["skills", "sync"]);
    let authored = f.installed(".shared/notes.md");
    write(&authored, "local notes");
    f.config(false);
    f.success(&["skills", "sync"]);
    assert!(
        !f.installed(".shared/upstream-example/references/check.md")
            .exists()
    );
    assert!(authored.exists());
    assert!(f.installed("upstream-example@alpha/SKILL.md").exists());
}

#[test]
fn invalid_resource_declarations_fail_before_installation() {
    for invalid in [
        "../escape.md",
        "/absolute.md",
        "references/*.md",
        "references/payload.sh",
        "references/SKILL.md",
        "references/missing.md",
    ] {
        let f = Fixture::new();
        let config = f.root.join("asobi.toml");
        let text = std::fs::read_to_string(&config).unwrap().replace(
            "[\"references/check.md\", \"references/nested/guide.md\"]",
            &format!("[{invalid:?}]"),
        );
        write(&config, &text);
        let result = f.run(&["skills", "sync"]);
        assert!(!result.status.success(), "accepted {invalid}");
        assert!(!f.installed("upstream-example@alpha").exists());
    }
}

#[test]
fn two_sources_keep_shared_names_and_scoped_operations_separate() {
    let f = Fixture::new();
    let second = f.root.join("other/example");
    write(
        &second.join("skills/gamma/SKILL.md"),
        "---\nname: gamma\n---\nRead `../../references/check.md`.\n",
    );
    write(&second.join("references/check.md"), "Second source.\n");
    let config = f.root.join("asobi.toml");
    let first_config = std::fs::read_to_string(&config).unwrap();
    write(
        &config,
        &format!(
            "{first_config}\n[[skills.source]]\nurl = {:?}\nselect = [\"gamma\"]\nshared_markdown = [\"references/check.md\"]\n",
            second.to_str().unwrap()
        ),
    );
    f.success(&["skills", "sync"]);
    let first_resource = f.installed(".shared/upstream-example/references/check.md");
    let second_resource = f.installed(".shared/other-example/references/check.md");
    assert_ne!(
        std::fs::read_to_string(&first_resource).unwrap(),
        std::fs::read_to_string(&second_resource).unwrap()
    );
    write(&f.source.join("references/check.md"), "First updated.\n");
    write(
        &second.join("references/check.md"),
        "Second upstream change.\n",
    );
    f.success(&["skills", "update", "upstream-example"]);
    assert_eq!(
        std::fs::read_to_string(&first_resource).unwrap(),
        "First updated.\n"
    );
    assert_eq!(
        std::fs::read_to_string(&second_resource).unwrap(),
        "Second source.\n"
    );
    let sibling_skill = f.installed("other-example@gamma/SKILL.md");
    let sibling_body = std::fs::read_to_string(&sibling_skill).unwrap();
    std::fs::remove_file(&sibling_skill).unwrap();
    write(
        &f.source.join("references/check.md"),
        "Must remain unapplied.\n",
    );
    assert!(
        !f.run(&["skills", "update", "upstream-example"])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read_to_string(&first_resource).unwrap(),
        "First updated.\n"
    );
    assert!(second_resource.exists());
    write(&sibling_skill, &sibling_body);
    f.success(&["skills", "remove", "upstream-example"]);
    assert!(!first_resource.exists());
    assert!(second_resource.exists());
    write(&config, &first_config);
    f.success(&["skills", "sync"]);
    assert!(first_resource.exists());
    assert!(!second_resource.exists());
}

#[test]
fn existing_unowned_destination_is_preserved() {
    let f = Fixture::new();
    let file = f.installed(".shared/upstream-example/references/check.md");
    write(&file, "Authored text.\n");
    let result = f.run(&["skills", "sync"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("ownership manifest"));
    assert_eq!(std::fs::read_to_string(file).unwrap(), "Authored text.\n");
    assert!(!f.installed("upstream-example@alpha").exists());
}

#[cfg(unix)]
#[test]
fn source_and_destination_symlink_ancestors_are_rejected() {
    use std::os::unix::fs::symlink;
    for destination in [false, true] {
        let f = Fixture::new();
        let outside = f.root.join("outside");
        write(&outside.join("check.md"), "private");
        let linked = if destination {
            f.installed(".shared")
        } else {
            f.source.join("references")
        };
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        if linked.exists() {
            std::fs::remove_dir_all(&linked).unwrap();
        }
        symlink(&outside, &linked).unwrap();
        assert!(!f.run(&["skills", "sync"]).status.success());
        assert_eq!(
            std::fs::read_to_string(outside.join("check.md")).unwrap(),
            "private"
        );
        assert!(!f.installed("upstream-example@alpha").exists());
    }
}

#[cfg(unix)]
#[test]
fn source_and_destination_symlinks_are_rejected_without_writes() {
    use std::os::unix::fs::symlink;
    for destination in [false, true] {
        let f = Fixture::new();
        let outside = f.root.join("outside.md");
        write(&outside, "private");
        let linked = if destination {
            f.installed(".shared/upstream-example/references/check.md")
        } else {
            f.source.join("references/check.md")
        };
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        if linked.exists() {
            std::fs::remove_file(&linked).unwrap();
        }
        symlink(&outside, &linked).unwrap();
        assert!(!f.run(&["skills", "sync"]).status.success());
        assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
        assert!(!f.installed("upstream-example@alpha").exists());
    }
}

#[test]
fn unsupported_reference_syntax_fails_instead_of_leaving_a_broken_pointer() {
    for reference in [
        "../../references/check.md",
        "./../../references/check.md",
        "./../../references/./check.md#gate",
    ] {
        let f = Fixture::new();
        write(
            &f.source.join("skills/alpha/SKILL.md"),
            &format!("---\nname: alpha\n---\n[guide]: {reference}\n"),
        );
        let result = f.run(&["skills", "sync"]);
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr)
                .contains("unsupported shared Markdown reference")
        );
        assert!(!f.installed("upstream-example@alpha").exists());
    }
}

#[test]
fn lost_manifest_and_missing_owned_files_fail_before_mutation() {
    for missing in ["manifest", "skill", "resource"] {
        let f = Fixture::new();
        f.success(&["skills", "sync"]);
        let skill = f.installed("upstream-example@alpha/SKILL.md");
        let resource = f.installed(".shared/upstream-example/references/check.md");
        let removed = match missing {
            "manifest" => f.root.join("state/skills.json"),
            "skill" => skill.clone(),
            _ => resource.clone(),
        };
        std::fs::remove_file(removed).unwrap();
        let remains = if missing == "resource" {
            &skill
        } else {
            &resource
        };
        let original = std::fs::read_to_string(remains).unwrap();
        write(&f.source.join("references/check.md"), "Changed upstream.\n");
        for args in [
            &["skills", "update"][..],
            &["skills", "sync"],
            &["skills", "remove", "alpha"],
        ] {
            assert!(!f.run(args).status.success(), "accepted missing {missing}");
            assert_eq!(std::fs::read_to_string(remains).unwrap(), original);
        }
    }
}

#[cfg(unix)]
#[test]
fn empty_removal_does_not_follow_a_linked_skills_root() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.config(false);
    f.success(&["skills", "sync"]);
    let dir = f.root.join(".agents/skills");
    let outside = f.root.join("outside-skills");
    std::fs::rename(&dir, &outside).unwrap();
    symlink(&outside, &dir).unwrap();
    assert!(!f.run(&["skills", "remove", "alpha"]).status.success());
    assert!(outside.join("upstream-example@alpha/SKILL.md").is_file());
}

#[test]
fn colliding_source_namespaces_fail_before_installation() {
    let f = Fixture::new();
    let second = f.root.join("nested/upstream/example");
    write(
        &second.join("skills/gamma/SKILL.md"),
        "---\nname: gamma\n---\nOther source.\n",
    );
    write(
        &second.join("references/check.md/nested.md"),
        "Other source resource.\n",
    );
    let config = f.root.join("asobi.toml");
    let first_config = std::fs::read_to_string(&config).unwrap();
    write(
        &config,
        &format!(
            "{first_config}\n[[skills.source]]\nurl = {:?}\nselect = [\"gamma\"]\nshared_markdown = [\"references/check.md/nested.md\"]\n",
            second.to_str().unwrap()
        ),
    );
    let result = f.run(&["skills", "sync"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("share namespace"));
    assert!(!f.installed("upstream-example@alpha").exists());
    assert!(!f.installed("upstream-example@gamma").exists());
}

#[cfg(unix)]
#[test]
fn bundled_destination_validation_precedes_any_installed_changes() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.success(&["skills", "sync"]);
    let skill = f.installed("upstream-example@alpha/SKILL.md");
    let original = std::fs::read_to_string(&skill).unwrap();
    let resource = f.installed(".shared/upstream-example/references/check.md");
    let original_resource = std::fs::read_to_string(&resource).unwrap();
    write(
        &f.source.join("skills/alpha/SKILL.md"),
        "---\nname: alpha\n---\nChanged body.\n",
    );
    write(&f.source.join("references/check.md"), "Changed resource.\n");
    let outside = f.root.join("outside.md");
    write(&outside, "private");
    let linked = f.installed("upstream-example@alpha/notes.md");
    std::fs::remove_file(&linked).unwrap();
    symlink(&outside, &linked).unwrap();
    assert!(!f.run(&["skills", "sync"]).status.success());
    assert_eq!(std::fs::read_to_string(skill).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(resource).unwrap(),
        original_resource
    );
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "private");
}

#[test]
fn old_manifest_without_source_policy_remains_readable_and_updatable() {
    let f = Fixture::new();
    f.config(false);
    f.success(&["skills", "sync"]);
    let file = f.root.join("state/skills.json");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    manifest.as_object_mut().unwrap().remove("resources");
    for skill in manifest["skills"].as_array_mut().unwrap() {
        skill.as_object_mut().unwrap().remove("sourceConfig");
    }
    write(&file, &serde_json::to_string(&manifest).unwrap());
    f.success(&["skills"]);
    f.success(&["skills", "update"]);
    assert!(f.installed("upstream-example@alpha/SKILL.md").exists());
    assert!(f.installed("upstream-example@beta/SKILL.md").exists());
}

#[test]
fn update_retains_git_revision_for_skills_and_shared_documents() {
    let f = Fixture::new();
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .current_dir(&f.source)
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-b", "main"]);
    git(&["add", "skills", "references"]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "fixture one"]);
    let pin = git(&["rev-parse", "HEAD"]);
    let url = format!("file://{}", f.source.display());
    let config = f.root.join("asobi.toml");
    let text = std::fs::read_to_string(&config).unwrap().replace(
        &format!("url = {:?}", f.source.to_str().unwrap()),
        &format!("url = {url:?}\nrev = {pin:?}"),
    );
    write(&config, &text);
    f.success(&["skills", "sync"]);
    let slug = asobi::normalize::slugify(&asobi::skills::derive_source_slug(&url));
    let shared = f.installed(&format!(".shared/{slug}/references/check.md"));
    let before = std::fs::read_to_string(&shared).unwrap();
    write(
        &f.source.join("references/check.md"),
        "New upstream revision.\n",
    );
    git(&["add", "references/check.md"]);
    git(&["-c", "commit.gpgsign=false", "commit", "-m", "fixture two"]);
    f.success(&["skills", "update"]);
    assert_eq!(std::fs::read_to_string(shared).unwrap(), before);
    assert!(!f.installed(&format!("{slug}@beta")).exists());
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.root.join("state/skills.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["skills"][0]["version"], pin);
    assert_eq!(manifest["resources"][0]["version"], pin);
}
