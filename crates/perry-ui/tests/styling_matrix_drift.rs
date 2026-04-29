//! Integration test: matrix vs reality.
//!
//! Runs the same drift check as `styling-matrix --check`, but as a
//! `cargo test` integration test so `cargo test --workspace` catches
//! drift directly without needing the shell script.

use std::path::PathBuf;

use perry_ui::styling_matrix::{drift, MATRIX};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at crates/perry-ui; up two levels = workspace root.
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set in test env");
    PathBuf::from(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .expect("could not resolve workspace root from CARGO_MANIFEST_DIR")
}

#[test]
fn matrix_matches_lib_rs_exports_on_every_native_platform() {
    let root = workspace_root();
    let drifts = drift::check_all(&root);

    let mut failures = String::new();
    let mut checked = 0usize;
    for d in &drifts {
        checked += 1;
        if d.is_clean() {
            continue;
        }
        let plat = d.platform.map(|p| p.name()).unwrap_or("?");
        failures.push_str(&format!("\n  {} drift:\n", plat));
        for s in &d.wired_but_missing {
            failures.push_str(&format!(
                "    matrix Wired/Stub but missing in lib.rs: {}\n",
                s
            ));
        }
        for s in &d.present_but_marked_missing {
            failures.push_str(&format!("    in lib.rs but matrix Missing/NA: {}\n", s));
        }
    }

    assert!(
        failures.is_empty(),
        "styling matrix is out of sync with lib.rs exports.\n\
         {} rows × {} native platforms checked.\n\
         Either update crates/perry-ui/src/styling_matrix.rs to match reality, or\n\
         update the affected backend's lib.rs to match the matrix.\n{}",
        MATRIX.len(),
        checked,
        failures
    );
}

#[test]
fn every_native_platform_has_at_least_one_styling_export() {
    // Sanity check: catches a misconfigured workspace where lib.rs scans
    // come back empty (e.g., the path is wrong) and every Wired cell
    // would silently report drift.
    let root = workspace_root();
    let drifts = drift::check_all(&root);
    for d in &drifts {
        if d.wired_but_missing.is_empty() {
            continue;
        }
        let plat = d
            .platform
            .expect("native drift entries always carry a platform");
        let path = plat
            .lib_rs_path()
            .expect("native platforms always have a lib.rs path");
        let exists = root.join(path).exists();
        assert!(
            exists,
            "lib.rs missing for {}: {} — is the workspace checkout complete?",
            plat.name(),
            path
        );
    }
}
