use std::process::Command;

#[test]
fn test_cli_version_matches_cargo_pkg_version() {
    let pkg_version = env!("CARGO_PKG_VERSION");

    // Build path to target binary
    let cargo_bin = env!("CARGO_BIN_EXE_miku");

    let output = Command::new(cargo_bin)
        .arg("--version")
        .output()
        .expect("Failed to run miku --version");

    assert!(output.status.success(), "miku --version failed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // miku <version> (e.g. miku 0.6.0)
    assert!(
        stdout.contains(pkg_version),
        "Expected version '{}' in output, got: '{}'",
        pkg_version,
        stdout
    );
}
