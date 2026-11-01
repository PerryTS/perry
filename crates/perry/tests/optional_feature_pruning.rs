//! Native acceptance coverage for receiver-aware regex and process IPC pruning.
#[test]
#[cfg(unix)]
fn optional_feature_pruning_preserves_signals_and_ipc() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = std::process::Command::new("python3")
        .arg(root.join("scripts/test-feature-pruning.py"))
        .env("PERRY_BIN", env!("CARGO_BIN_EXE_perry"))
        .current_dir(&root)
        .output()
        .expect("run bounded feature pruning regression");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
