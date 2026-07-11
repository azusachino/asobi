use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

#[test]
fn test_concurrency_lock_storm() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("concurrency_test.db");
    let db_path_str = db_path.to_str().unwrap();

    // Find the asobi binary
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

    // 1. Create the entity first so it exists
    let status = Command::new(&bin_path)
        .arg("new")
        .arg("concurrent_entity")
        .arg("concept")
        .env("ASOBI_DATABASE_URL", db_path_str)
        .status()
        .expect("failed to execute asobi new");
    assert!(
        status.success(),
        "Failed to create entity 'concurrent_entity'"
    );

    // 2. Spawn concurrent processes
    let num_threads = 12;
    let barrier = Arc::new(Barrier::new(num_threads));
    let bin_path = Arc::new(bin_path);
    let db_path_str = Arc::new(db_path_str.to_string());
    let mut handles = vec![];

    for i in 0..num_threads {
        let barrier_clone = barrier.clone();
        let bin_path_clone = bin_path.clone();
        let db_path_str_clone = db_path_str.clone();

        let handle = thread::spawn(move || {
            // Synchronize starting point
            barrier_clone.wait();

            for j in 0..10 {
                let output = if i % 2 == 0 {
                    // Writer
                    Command::new(bin_path_clone.as_ref())
                        .arg("obs")
                        .arg("concurrent_entity")
                        .arg(format!("thread {} iteration {}", i, j))
                        .env("ASOBI_DATABASE_URL", db_path_str_clone.as_ref())
                        .env("RUST_BACKTRACE", "1")
                        .output()
                } else {
                    // Reader / Truth upsert
                    if j % 2 == 0 {
                        Command::new(bin_path_clone.as_ref())
                            .arg("show")
                            .arg("concurrent_entity")
                            .env("ASOBI_DATABASE_URL", db_path_str_clone.as_ref())
                            .env("RUST_BACKTRACE", "1")
                            .output()
                    } else {
                        Command::new(bin_path_clone.as_ref())
                            .arg("truth")
                            .arg("concurrent_entity")
                            .arg(format!("key_{}", i))
                            .arg(format!("val_{}", j))
                            .env("ASOBI_DATABASE_URL", db_path_str_clone.as_ref())
                            .env("RUST_BACKTRACE", "1")
                            .output()
                    }
                };

                let output = output.expect("failed to execute asobi command");
                assert!(
                    output.status.success(),
                    "command failed in thread {i} iteration {j}\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        });
        handles.push(handle);
    }

    let mut failed = 0;
    for handle in handles {
        if handle.join().is_err() {
            failed += 1;
        }
    }

    assert_eq!(failed, 0, "{} threads failed execution", failed);
}
