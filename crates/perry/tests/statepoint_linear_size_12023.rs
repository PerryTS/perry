//! The #11926 workload must remain linear through machine emission and map
//! encoding. This invokes the same persisted size gate used for the q200
//! sabotage experiment; no compiler-internal estimate substitutes for bytes.
#![cfg(all(target_arch = "x86_64", target_os = "linux"))]

use std::path::Path;
use std::process::Command;

#[test]
fn statepoint_machine_code_and_gc_maps_scale_linearly() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let dir = tempfile::Builder::new()
        .prefix("sp-linear-size-test-")
        .tempdir()
        .unwrap();
    let output = Command::new("python3")
        .arg(workspace.join("scripts/check_statepoint_linear_size.py"))
        .arg("--perry")
        .arg(env!("CARGO_BIN_EXE_perry"))
        .arg("--work-dir")
        .arg(dir.path())
        .env("PERRY_WORKSPACE_ROOT", workspace)
        .output()
        .expect("run statepoint size gate");
    assert!(
        output.status.success(),
        "statepoint machine/map size regression:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
