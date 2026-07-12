use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_cli_search_where_only() {
    // 1. Setup temp database
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap();

    // 2. Find the asobi binary
    let mut bin_path = std::env::current_exe().unwrap();
    bin_path.pop(); // remove filename
    if bin_path.ends_with("deps") {
        bin_path.pop(); // remove deps
    }
    bin_path.push("asobi");

    assert!(
        bin_path.exists(),
        "Asobi binary not found at {:?}",
        bin_path
    );

    // 3. Create entity 'task-1'
    let status = Command::new(&bin_path)
        .arg("new")
        .arg("task-1")
        .arg("task")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 4. Set status truth for 'task-1' to 'READY'
    let status = Command::new(&bin_path)
        .arg("truth")
        .arg("task-1")
        .arg("status")
        .arg("READY")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 5. Create entity 'task-2'
    let status = Command::new(&bin_path)
        .arg("new")
        .arg("task-2")
        .arg("task")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 6. Set status truth for 'task-2' to 'BLOCKED'
    let status = Command::new(&bin_path)
        .arg("truth")
        .arg("task-2")
        .arg("status")
        .arg("BLOCKED")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 7. Search only by status=READY (query omitted)
    let output = Command::new(&bin_path)
        .arg("search")
        .arg("--where")
        .arg("status=READY")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");

    assert!(
        output.status.success(),
        "Expected search command to succeed, but it failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let envelope: serde_json::Value = serde_json::from_str(&stdout_str).unwrap();
    let graph = &envelope["data"];

    let entities = graph["entities"]
        .as_array()
        .expect("entities field missing or not an array");

    // Should only match task-1
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0]["name"], "task-1");
}
