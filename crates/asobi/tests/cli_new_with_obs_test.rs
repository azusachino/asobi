use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_cli_new_with_obs() {
    // 1. Setup temp database
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap();

    // 2. Find the asobi binary. We assume it is compiled at target/debug/asobi
    let mut bin_path = std::env::current_exe().unwrap();
    // Navigate up from target/debug/deps/cli_new_with_obs_test-XXXX to target/debug/asobi
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

    // 3. Create entity 'foo' with seeded observations
    let output = Command::new(&bin_path)
        .arg("new")
        .arg("foo")
        .arg("concept")
        .arg("--obs")
        .arg("first observation")
        .arg("--obs")
        .arg("second observation")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");

    assert!(
        output.status.success(),
        "Expected new command with --obs to succeed, but it failed.\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    // 4. Verify observations using show
    let output = Command::new(&bin_path)
        .arg("show")
        .arg("foo")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");

    assert!(output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let graph: serde_json::Value = serde_json::from_str(&stdout_str).unwrap();

    // The returned JSON structure from show contains entities
    let entities = graph["entities"]
        .as_array()
        .expect("entities field missing or not an array");
    assert_eq!(entities.len(), 1);
    let entity = &entities[0];
    assert_eq!(entity["name"], "foo");

    let observations = entity["observations"]
        .as_array()
        .expect("observations field missing or not an array");

    let mut obs_strs: Vec<&str> = observations.iter().map(|v| v.as_str().unwrap()).collect();
    obs_strs.sort();

    assert_eq!(obs_strs, vec!["first observation", "second observation"]);
}
