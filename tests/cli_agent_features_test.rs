use std::process::Command;
use tempfile::tempdir;

#[test]
fn test_cli_agent_features() {
    // 1. Setup temp database
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let db_path_str = db_path.to_str().unwrap();

    // 2. Find the asobi binary
    let mut bin_path = std::env::current_exe().unwrap();
    bin_path.pop();
    if bin_path.ends_with("deps") {
        bin_path.pop();
    }
    bin_path.push("asobi");

    assert!(
        bin_path.exists(),
        "Asobi binary not found at {:?}",
        bin_path
    );

    // 3. Create entities 'alice' and 'bob'
    let status = Command::new(&bin_path)
        .arg("new")
        .arg("alice")
        .arg("person")
        .arg("bob")
        .arg("person")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 4. Add observations to 'alice'
    let status = Command::new(&bin_path)
        .arg("obs")
        .arg("alice")
        .arg("status: active")
        .arg("next: write tests")
        .arg("old details here")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 5. Create a relation 'alice' -> 'bob' (follows)
    let status = Command::new(&bin_path)
        .arg("link")
        .arg("alice")
        .arg("bob")
        .arg("follows")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 6. Test rm-obs with --prefix
    let status = Command::new(&bin_path)
        .arg("rm-obs")
        .arg("alice")
        .arg("status:")
        .arg("--prefix")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 7. Test update-obs
    let status = Command::new(&bin_path)
        .arg("update-obs")
        .arg("alice")
        .arg("old details here")
        .arg("new details here")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi");
    assert!(status.success());

    // 8. Test show --with-timestamps
    let output = Command::new(&bin_path)
        .arg("show")
        .arg("alice")
        .arg("--with-timestamps")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");
    assert!(output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let graph: serde_json::Value = serde_json::from_str(&stdout_str).unwrap();
    let alice = &graph["entities"][0];
    assert_eq!(alice["name"], "alice");
    let detailed_obs = alice["observationsDetailed"]
        .as_array()
        .expect("detailed observations missing");

    // We deleted "status: active" via prefix, and updated "old details here" to "new details here".
    // So observations should be "next: write tests" and "new details here".
    let mut contents: Vec<&str> = detailed_obs
        .iter()
        .map(|v| v["content"].as_str().unwrap())
        .collect();
    contents.sort();
    assert_eq!(contents, vec!["new details here", "next: write tests"]);
    // Verify created_at is present
    assert!(detailed_obs[0]["createdAt"].is_string());

    // 9. Test show --expand
    // By showing alice and expanding 'follows', bob should also be loaded!
    let output = Command::new(&bin_path)
        .arg("show")
        .arg("alice")
        .arg("--expand")
        .arg("follows")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");
    assert!(output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let graph: serde_json::Value = serde_json::from_str(&stdout_str).unwrap();
    let entities = graph["entities"].as_array().unwrap();
    let mut names: Vec<&str> = entities
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, vec!["alice", "bob"]);

    // 10. Test stats --per-entity
    let output = Command::new(&bin_path)
        .arg("stats")
        .arg("--per-entity")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");
    assert!(output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    assert!(stdout_str.contains("Entities by Observation Count:"));
    assert!(stdout_str.contains("alice"));

    // 10b. Test stats --json --per-entity
    let output = Command::new(&bin_path)
        .arg("--json")
        .arg("stats")
        .arg("--per-entity")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");
    assert!(output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let stats_json: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("Expected stats to be JSON");
    assert_eq!(stats_json["entities"], 2);
    assert_eq!(stats_json["relations"], 1);
    let detailed = stats_json["entitiesDetailed"].as_array().unwrap();
    assert_eq!(detailed[0]["name"], "alice");

    // 11. Test JSON error formatting when --json is set globally
    let output = Command::new(&bin_path)
        .arg("--json")
        .arg("import")
        .arg("nonexistent_file_xyz.json")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .output()
        .expect("failed to execute asobi");
    assert!(!output.status.success());
    let stdout_str = String::from_utf8(output.stdout).unwrap();
    let err_json: serde_json::Value =
        serde_json::from_str(&stdout_str).expect("Expected stdout to be JSON error");
    assert_eq!(err_json["status"], "failed");
    assert!(
        err_json["error"]
            .as_str()
            .unwrap()
            .contains("No such file or directory")
    );
}
