use std::process::Command;

#[test]
fn test_cli_version_matches_cargo_pkg_version() {
    let pkg_version = env!("CARGO_PKG_VERSION");

    // Build path to target binary
    let cargo_bin = env!("CARGO_BIN_EXE_asobi");

    let output = Command::new(cargo_bin)
        .arg("--version")
        .output()
        .expect("Failed to run asobi --version");

    assert!(output.status.success(), "asobi --version failed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // asobi <version> (e.g. asobi 0.6.0)
    assert!(
        stdout.contains(pkg_version),
        "Expected version '{}' in output, got: '{}'",
        pkg_version,
        stdout
    );
}
