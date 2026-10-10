use super::*;

fn checkout(root: &Path, version: &str) {
    fs::create_dir_all(root.join("crates/perry-runtime")).unwrap();
    fs::create_dir_all(root.join("crates/perry-ui-geisterhand")).unwrap();
    fs::create_dir_all(root.join("crates/perry")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        format!("[workspace.package]\nversion = {version:?}\n"),
    )
    .unwrap();
    fs::write(
        root.join("crates/perry/Cargo.toml"),
        "[package]\nname = 'perry'\n",
    )
    .unwrap();
}

#[test]
fn external_target_and_unrelated_cwd_find_build_checkout() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("source");
    checkout(&root, env!("CARGO_PKG_VERSION"));
    let exe = fixture.path().join("external-target/release/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "compiler").unwrap();
    let cwd = fixture.path().join("application");
    fs::create_dir(&cwd).unwrap();
    assert_eq!(
        workspace_root_from_locations(Some(&exe), Some(&cwd), &root),
        Some(root.canonicalize().unwrap()),
        "external Cargo targets must not lose their build checkout"
    );
}

#[test]
fn stale_or_nonmatching_build_checkout_is_ignored() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("source");
    assert!(workspace_root_from_locations(None, None, &root).is_none());
    checkout(&root, "999.0.0");
    assert!(workspace_root_from_locations(None, None, &root).is_none());
    checkout(&root, env!("CARGO_PKG_VERSION"));
    fs::write(
        root.join("crates/perry/Cargo.toml"),
        "[package]\nname = 'other'\n",
    )
    .unwrap();
    assert!(workspace_root_from_locations(None, None, &root).is_none());
    fs::write(root.join("Cargo.toml"), "invalid toml").unwrap();
    assert!(workspace_root_from_locations(None, None, &root).is_none());
}

#[test]
fn exe_checkout_precedes_build_record_and_cwd_remains_a_fallback() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path().join("source");
    checkout(&root, env!("CARGO_PKG_VERSION"));
    let moved = fixture.path().join("moved");
    checkout(&moved, env!("CARGO_PKG_VERSION"));
    let exe = moved.join("target/release/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, "compiler").unwrap();
    assert_eq!(
        workspace_root_from_locations(Some(&exe), None, &root),
        Some(moved.canonicalize().unwrap())
    );
    assert_eq!(
        workspace_root_from_locations(None, Some(&moved), &fixture.path().join("gone")),
        Some(moved)
    );
}
