use std::path::Path;

#[test]
fn legacy_ffi_surface_stays_removed() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(crate_dir.join("Cargo.toml"))
        .expect("read perry-container-compose Cargo.toml");
    let features = manifest
        .split_once("[features]")
        .expect("manifest has a features table")
        .1
        .split("\n[")
        .next()
        .expect("features table has contents");

    assert!(
        !features.lines().any(|line| {
            line.split('#')
                .next()
                .is_some_and(|entry| entry.trim_start().starts_with("ffi ="))
        }),
        "the legacy `ffi` feature must not be reintroduced; perry-stdlib owns the canonical compose FFI"
    );

    let lib = std::fs::read_to_string(crate_dir.join("src/lib.rs"))
        .expect("read perry-container-compose src/lib.rs");
    assert!(
        !lib.contains("feature = \"ffi\""),
        "src/lib.rs must not gate a module on the removed legacy `ffi` feature"
    );
    assert!(
        !crate_dir.join("src/ffi.rs").exists(),
        "the duplicate legacy compose FFI module must stay deleted"
    );
}
