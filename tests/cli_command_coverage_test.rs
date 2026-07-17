use std::process::Command;

fn asobi() -> &'static str {
    env!("CARGO_BIN_EXE_asobi")
}

#[test]
fn every_top_level_and_nested_subcommand_has_help() {
    let top_level = [
        "new",
        "link",
        "obs",
        "truth",
        "rm-truth",
        "history",
        "rm",
        "rm-obs",
        "update-obs",
        "unlink",
        "search",
        "show",
        "compact",
        "purge",
        "init",
        "stats",
        "schema",
        "export",
        "import",
        "reset",
        "backup",
        "restore",
        "completions",
        "skills",
        "tasks",
    ];
    for command in top_level {
        let output = Command::new(asobi())
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "help failed for {command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for args in [
        ["skills", "install", "--help"],
        ["skills", "update", "--help"],
        ["skills", "remove", "--help"],
        ["skills", "show", "--help"],
        ["tasks", "plan", "--help"],
        ["tasks", "list", "--help"],
        ["tasks", "dispatch", "--help"],
        ["tasks", "sync", "--help"],
        ["tasks", "close", "--help"],
    ] {
        let output = Command::new(asobi()).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "help failed for {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn completions_generate_for_supported_shells() {
    for shell in ["bash", "elvish", "fish", "powershell", "zsh"] {
        let output = Command::new(asobi())
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "completion generation failed for {shell}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "empty completion script for {shell}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("asobi"),
            "completion script does not mention asobi for {shell}"
        );
    }
}

#[test]
fn unknown_subcommands_fail_cleanly() {
    let output = Command::new(asobi())
        .args(["not-a-command", "--help"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand"));
}
