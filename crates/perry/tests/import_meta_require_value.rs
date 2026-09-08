//! CI-visible entry point for the bounded standalone native regression.
use std::{path::Path, process::Command};

#[test]
fn standalone_regression() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .arg(root.join("scripts/test-import-meta-require-value.mjs"))
        .env("PERRY_BIN", env!("CARGO_BIN_EXE_perry"))
        .env("PERRY_WORKSPACE_ROOT", &root)
        .env("PERRY_TEST_BUILD_RUNTIME", "1")
        .current_dir(&root)
        .output()
        .expect("run bounded Node regression driver");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
