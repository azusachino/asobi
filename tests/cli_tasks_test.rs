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
