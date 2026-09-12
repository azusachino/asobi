use std::path::Path;
use std::process::Command;

fn write(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
}

fn sync(root: &Path) -> String {
    let result = Command::new(env!("CARGO_BIN_EXE_asobi"))
        .current_dir(root)
        .env_remove("ASOBI_HOME")
        .env("ASOBI_DATABASE_URL", root.join("state/asobi.db"))
        .args(["skills", "sync"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stderr).unwrap()
}

#[test]
fn warns_about_unresolved_companions_and_skill_invocations_without_installing_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("upstream/example");
    write(
        &source.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\nRead [glossary](GLOSSARY.md#terms), `references/missing.md`, `../../outside.md`, `../local-directory/linked.md`, and [script](scripts/run.sh). Run a `/grilling` session using /domain-modeling.\n",
    );
    write(
        &source.join("grilling/SKILL.md"),
        "---\nname: grilling\n---\nOther skill.\n",
    );
    write(
        &source.join("alpha/scripts/run.sh"),
        "must not run or install\n",
    );
    #[cfg(unix)]
    {
        write(
            &root.join(".agents/skills/local-directory/SKILL.md"),
            "---\nname: local\n---\nLocal skill.\n",
        );
        write(&root.join("outside.md"), "private fixture contents\n");
        std::os::unix::fs::symlink(
            root.join("outside.md"),
            root.join(".agents/skills/local-directory/linked.md"),
        )
        .unwrap();
    }
    write(
        &root.join("asobi.toml"),
        &format!(
            "data_dir = 'state'\n[[skills.source]]\nurl = {:?}\nselect = ['alpha']\n",
            source.to_str().unwrap()
        ),
    );
    let diagnostic = sync(&root);
    for reference in [
        "GLOSSARY.md#terms",
        "references/missing.md",
        "../../outside.md",
        "../local-directory/linked.md",
        "scripts/run.sh",
        "/grilling",
        "/domain-modeling",
    ] {
        assert!(
            diagnostic.lines().any(|line| line
                .contains("cannot resolve from planned installation")
                && line.contains(reference)),
            "missing diagnostic for {reference}: {diagnostic}"
        );
    }
    assert!(
        !root
            .join(".agents/skills/upstream-example@grilling")
            .exists()
    );
    assert!(
        !root
            .join(".agents/skills/upstream-example@alpha/scripts/run.sh")
            .exists()
    );
}

#[test]
fn planned_companions_shared_files_and_cross_source_skills_resolve() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("upstream/example");
    let other = root.join("other/example");
    write(
        &source.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\nRead [glossary](GLOSSARY.md#terms), `../references/shared.md`, [local](../local-directory/SKILL.md), and `../local-directory/references/guide.md`. Run `/beta`, /other-example@beta and `/local-helper`.\n",
    );
    write(&source.join("alpha/GLOSSARY.md"), "# Terms\n");
    write(&source.join("references/shared.md"), "Shared text.\n");
    write(
        &other.join("beta/SKILL.md"),
        "---\nname: beta\n---\nOther skill.\n",
    );
    write(
        &root.join(".agents/skills/local-directory/SKILL.md"),
        "---\nname: local-helper\n---\nLocal helper.\n",
    );
    write(
        &root.join(".agents/skills/local-directory/references/guide.md"),
        "Local companion.\n",
    );
    write(
        &root.join("asobi.toml"),
        &format!(
            "data_dir = 'state'\n[[skills.source]]\nurl = {:?}\nselect = ['alpha']\nshared_markdown = ['references/shared.md']\n[[skills.source]]\nurl = {:?}\nselect = ['beta']\n",
            source.to_str().unwrap(),
            other.to_str().unwrap()
        ),
    );
    assert!(!sync(&root).contains("cannot resolve from planned installation"));
}

#[test]
fn urls_anchors_and_command_examples_are_not_dependencies() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("upstream/example");
    write(
        &source.join("alpha/SKILL.md"),
        "---\nname: alpha\n---\n[web](https://example.invalid/missing.md) [anchor](#intro) `/bin/bash` `make check /tmp` `cargo test`\n```sh\n/grilling [file](missing.md)\n```\n    /domain-modeling [file](missing.md)\n````markdown\n```sh\n/grilling [file](missing.md)\n```\n~~~\n/domain-modeling [file](missing.md)\n````not-a-close\n/grilling [file](missing.md)\n`````   \nRun /after-example.\n",
    );
    write(
        &root.join("asobi.toml"),
        &format!(
            "data_dir = 'state'\n[[skills.source]]\nurl = {:?}\nselect = ['alpha']\n",
            source.to_str().unwrap()
        ),
    );
    let diagnostic = sync(&root);
    let warnings: Vec<_> = diagnostic
        .lines()
        .filter(|line| line.contains("cannot resolve from planned installation"))
        .collect();
    assert_eq!(warnings.len(), 1, "{diagnostic}");
    assert!(warnings[0].contains("/after-example"), "{diagnostic}");
}
