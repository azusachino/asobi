use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

fn asobi() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_asobi"))
}

#[test]
fn task_dispatcher_commands_and_help_work() {
    for args in [
        vec!["--help"],
        vec!["tasks", "--help"],
        vec!["tasks", "plan", "--help"],
        vec!["tasks", "list", "--help"],
        vec!["tasks", "claim", "--help"],
        vec!["tasks", "update", "--help"],
        vec!["tasks", "dispatch", "--help"],
        vec!["tasks", "sync", "--help"],
        vec!["tasks", "close", "--help"],
    ] {
        let output = Command::new(asobi()).args(args).output().unwrap();
        assert!(output.status.success(), "help failed: {output:?}");
    }

    let dir = tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let db = db.to_str().unwrap();

    let output = Command::new(asobi())
        .args([
            "tasks",
            "plan",
            "asobi:cli",
            "--objective",
            "Add task dispatching",
            "--task",
            "Implement commands",
            "--task",
            "Add integration tests",
        ])
        .env("ASOBI_DATABASE_URL", db)
        .output()
        .unwrap();
    assert!(output.status.success(), "plan failed: {output:?}");

    let output = Command::new(asobi())
        .args(["tasks", "list", "asobi:cli"])
        .env("ASOBI_DATABASE_URL", db)
        .output()
        .unwrap();
    assert!(output.status.success());
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let task = graph["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["name"] == "asobi:cli:task-1")
        .unwrap();
    assert_eq!(task["truths"]["status"], "READY_TO_DISPATCH");

    let output = Command::new(asobi())
        .args(["tasks", "dispatch"])
        .env("ASOBI_DATABASE_URL", db)
        .output()
        .unwrap();
    assert!(output.status.success(), "dispatch failed: {output:?}");

    let output = Command::new(asobi())
        .args([
            "tasks",
            "sync",
            "asobi:cli:task-1",
            "--note",
            "implementation complete",
            "--status",
            "DONE",
        ])
        .env("ASOBI_DATABASE_URL", db)
        .output()
        .unwrap();
    assert!(output.status.success(), "sync failed: {output:?}");
}

#[test]
fn named_claim_and_note_only_update_do_not_advance_status() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let run = |args: &[&str]| {
        Command::new(asobi())
            .args(args)
            .current_dir(dir.path())
            .env_remove("ASOBI_REMOTE")
            .env("ASOBI_DATABASE_URL", &db)
            .output()
            .unwrap()
    };
    assert!(
        run(&[
            "tasks",
            "plan",
            "asobi:handoff",
            "--objective",
            "Test handoff",
            "--task",
            "Review changes"
        ])
        .status
        .success()
    );
    assert!(
        !run(&["tasks", "claim"]).status.success(),
        "claim needs a named task"
    );
    assert!(
        run(&[
            "tasks",
            "claim",
            "asobi:handoff:task-1",
            "--agent",
            "reviewer"
        ])
        .status
        .success()
    );
    assert!(
        !run(&["tasks", "update", "asobi:handoff:task-1"])
            .status
            .success(),
        "empty update must fail"
    );
    let output = run(&[
        "--json",
        "tasks",
        "update",
        "asobi:handoff:task-1",
        "--note",
        "tests passed",
    ]);
    assert!(output.status.success(), "{output:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["status"], "DISPATCHED");
    let task = run(&["show", "asobi:handoff:task-1"]);
    let graph: serde_json::Value = serde_json::from_slice(&task.stdout).unwrap();
    assert_eq!(graph["entities"][0]["truths"]["status"], "DISPATCHED");
    assert!(
        graph["entities"][0]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note == "tests passed")
    );
    let output = run(&[
        "--json",
        "tasks",
        "update",
        "asobi:handoff:task-1",
        "--status",
        "DONE",
        "--note",
        "verified",
    ]);
    assert!(output.status.success(), "{output:?}");
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["status"], "DONE");
    let task = run(&["show", "asobi:handoff:task-1"]);
    let graph: serde_json::Value = serde_json::from_slice(&task.stdout).unwrap();
    assert_eq!(graph["entities"][0]["truths"]["status"], "DONE");
    assert!(
        graph["entities"][0]["observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|note| note == "verified")
    );
}

#[test]
fn file_backed_observation_and_task_note_preserve_literal_text() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let file = dir.path().join("handoff.txt");
    let content = "Long handoff: 'quotes' and 雪\nSecond line.\n";
    std::fs::write(&file, content).unwrap();
    let file = file.to_str().unwrap();
    let run = |args: &[&str]| {
        Command::new(asobi())
            .args(args)
            .current_dir(dir.path())
            .env_remove("ASOBI_REMOTE")
            .env("ASOBI_DATABASE_URL", &db)
            .output()
            .unwrap()
    };
    assert!(run(&["new", "asobi:file", "task"]).status.success());
    assert!(!run(&["obs", "asobi:file"]).status.success());
    let empty = dir.path().join("empty.txt");
    std::fs::write(&empty, " \n").unwrap();
    assert!(
        !run(&["obs", "asobi:file", "--file", empty.to_str().unwrap()])
            .status
            .success()
    );
    assert!(
        !run(&[
            "tasks",
            "update",
            "asobi:file",
            "--note-file",
            "missing.txt"
        ])
        .status
        .success()
    );
    assert!(run(&["obs", "asobi:file", "--file", file]).status.success());
    assert!(
        !run(&["obs", "asobi:file", "inline", "--file", file])
            .status
            .success()
    );
    let update = run(&[
        "--json",
        "tasks",
        "update",
        "asobi:file",
        "--note-file",
        file,
        "--status",
        "REVIEW",
    ]);
    assert!(update.status.success(), "{update:?}");
    assert!(
        !run(&[
            "tasks",
            "update",
            "asobi:file",
            "--note",
            "inline",
            "--note-file",
            file,
        ])
        .status
        .success()
    );
    let shown = run(&["show", "asobi:file"]);
    let graph: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(graph["entities"][0]["truths"]["status"], "REVIEW");
    assert_eq!(
        graph["entities"][0]["observations"],
        serde_json::json!([content, content])
    );
    assert!(
        !shown
            .stdout
            .windows(file.len())
            .any(|part| part == file.as_bytes())
    );

    let mut child = Command::new(asobi())
        .args(["obs", "asobi:file", "--file", "-"])
        .current_dir(dir.path())
        .env_remove("ASOBI_REMOTE")
        .env("ASOBI_DATABASE_URL", &db)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"from stdin\n")
        .unwrap();
    assert!(child.wait().unwrap().success());
    let shown = run(&["show", "asobi:file"]);
    let graph: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(graph["entities"][0]["observations"][2], "from stdin\n");
}

#[test]
fn only_one_concurrent_dispatcher_claims_a_task() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("contended-tasks.db");
    let db = db.to_str().unwrap().to_owned();
    let output = Command::new(asobi())
        .args([
            "tasks",
            "plan",
            "asobi:contention",
            "--objective",
            "Claim exactly once",
            "--task",
            "One winner",
        ])
        .env("ASOBI_DATABASE_URL", &db)
        .output()
        .unwrap();
    assert!(output.status.success(), "plan failed: {output:?}");

    let workers = 8;
    let barrier = Arc::new(Barrier::new(workers));
    let db = Arc::new(db);
    let mut handles = Vec::new();
    for index in 0..workers {
        let barrier = Arc::clone(&barrier);
        let db = Arc::clone(&db);
        handles.push(thread::spawn(move || {
            barrier.wait();
            Command::new(asobi())
                .args(["tasks", "dispatch", "--agent", &format!("agent-{index}")])
                .env("ASOBI_DATABASE_URL", db.as_ref())
                .output()
                .unwrap()
                .status
                .success()
        }));
    }

    let winners = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|success| *success)
        .count();
    assert_eq!(winners, 1, "exactly one agent must claim the task");
}
