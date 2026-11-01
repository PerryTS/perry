use super::*;

fn write_pkg(root: &Path, version: &str) {
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(
        root.join("package.json"),
        format!("{{\"name\":\"x\",\"version\":\"{version}\"}}"),
    )
    .unwrap();
}

#[test]
fn ext_package_is_found_beside_and_above_the_host_package() {
    let tmp = tempfile::tempdir().unwrap();
    let nm = tmp.path().join("lib/node_modules");
    let host_bin = nm.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin");
    std::fs::create_dir_all(&host_bin).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    // Global install of the ext package: the outer node_modules.
    let outer = nm.join("@perryts/perry-ext-linux-x64");
    write_pkg(&outer, version);
    let mut c = Vec::new();
    push_npm_ext_package_candidates(&mut c, &host_bin, "libperry_ext_http.a");
    assert_eq!(c, vec![outer.join("lib/libperry_ext_http.a")]);
    // A nested copy wins (nearest first) and both are offered.
    let inner = nm.join("@perryts/perry/node_modules/@perryts/perry-ext-linux-x64");
    write_pkg(&inner, version);
    let mut c = Vec::new();
    push_npm_ext_package_candidates(&mut c, &host_bin, "libperry_ext_http.a");
    assert_eq!(
        c,
        vec![
            inner.join("lib/libperry_ext_http.a"),
            outer.join("lib/libperry_ext_http.a")
        ]
    );
}

#[test]
fn ext_package_of_another_version_or_other_archives_add_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let nm = tmp.path().join("node_modules");
    let host_bin = nm.join("@perryts/perry-linux-x64/bin");
    std::fs::create_dir_all(&host_bin).unwrap();
    write_pkg(&nm.join("@perryts/perry-ext-linux-x64"), "0.0.1-other");
    let mut c = Vec::new();
    push_npm_ext_package_candidates(&mut c, &host_bin, "libperry_ext_http.a");
    assert!(c.is_empty(), "{c:?}");
    write_pkg(
        &nm.join("@perryts/perry-ext-linux-x64"),
        env!("CARGO_PKG_VERSION"),
    );
    push_npm_ext_package_candidates(&mut c, &host_bin, "libperry_runtime.a");
    assert!(
        c.is_empty(),
        "core archives never come from the ext package"
    );
}

#[test]
fn only_npm_host_package_layouts_pair_with_an_ext_package() {
    assert!(npm_ext_package(Path::new("/opt/perry/bin")).is_none());
    assert!(npm_ext_package(Path::new("/x/node_modules/@other/perry-linux-x64/bin")).is_none());
    let (_, name) = npm_ext_package(Path::new(
        "/x/node_modules/@perryts/perry-linux-x64-musl/bin",
    ))
    .unwrap();
    assert_eq!(name, "perry-ext-linux-x64-musl");
}
