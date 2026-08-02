use super::collect_library_candidates;
use std::path::PathBuf;

#[test]
fn macos_target_probes_bare_target_dirs() {
    let cands = collect_library_candidates("libperry_ui_macos.a", Some("macos"));
    // Triple-qualified dir is still searched — derive the triple from the
    // same mapping the production code uses rather than hardcoding it, so
    // this stays in sync if the `macos` → triple mapping ever changes.
    let triple =
        super::rust_target_triple(Some("macos")).expect("macos target should resolve to a triple");
    assert!(
        cands.contains(&PathBuf::from(format!(
            "target/{triple}/release/libperry_ui_macos.a"
        ))),
        "missing triple dir in {cands:?}"
    );
    // ...and so is the bare host-native dir the suggested build command writes to.
    assert!(
        cands.contains(&PathBuf::from("target/release/libperry_ui_macos.a")),
        "missing bare target/release in {cands:?}"
    );
    assert!(
        cands.contains(&PathBuf::from("target/debug/libperry_ui_macos.a")),
        "missing bare target/debug in {cands:?}"
    );
}

#[test]
fn cross_target_keeps_conservative_search() {
    // A real cross-compile (iOS) must NOT fall back to the host's bare
    // target/release — that would risk linking a macOS lib into an iOS app.
    let cands = collect_library_candidates("libperry_ui_ios.a", Some("ios"));
    assert!(
        !cands.contains(&PathBuf::from("target/release/libperry_ui_ios.a")),
        "cross target unexpectedly probed bare target/release in {cands:?}"
    );
}
