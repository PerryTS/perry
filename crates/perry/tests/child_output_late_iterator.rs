//! CI-visible native regression for late async readers of child output pipes.
use std::{path::Path, process::Command};

#[test]
fn standalone_regression() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("node")
        .arg(root.join("scripts/test-child-output-late-iterator.mjs"))
        .env("PERRY_BIN", env!("CARGO_BIN_EXE_perry"))
        .env("PERRY_WORKSPACE_ROOT", &root)
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
