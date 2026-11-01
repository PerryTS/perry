//! Search step 4: target and ext packages come after every install layout,
//! and only a root whose target.json matches this perry adds candidates.

use super::super::target_packages::{
    package_name, perry_home_root, PackageKind, PerryIdentity, TargetFile, TargetJson, HOST_TRIPLE,
    TARGET_JSON,
};
use super::{collect_library_candidates_from, locate_among};
use std::fs;
use std::path::{Path, PathBuf};

const LIB: &str = "libperry_runtime.a";

fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

/// A package root for `triple` declaring `version` and this binary's revision.
fn package(root: &Path, triple: &str, version: &str) -> PathBuf {
    fs::create_dir_all(root.join("lib")).unwrap();
    let me = PerryIdentity::current();
    let manifest = TargetJson {
        schema_version: 1,
        perry_version: version.into(),
        source_revision: me
            .revision
            .unwrap_or("0000000000000000000000000000000000000000")
            .into(),
        target_triple: triple.into(),
        files: vec![TargetFile {
            path: format!("lib/{LIB}"),
            sha256: "0".repeat(64),
            size: 0,
        }],
    };
    fs::write(
        root.join(TARGET_JSON),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    root.to_path_buf()
}

fn npm_dir(modules: &Path, triple: &str) -> PathBuf {
    modules.join(package_name(PackageKind::Target, triple))
}

fn npm_ext_dir(modules: &Path, triple: &str) -> PathBuf {
    modules.join(package_name(PackageKind::Ext, triple))
}

fn exe_in(modules: &Path) -> PathBuf {
    let exe = modules.join("@perryts/perry-linux-x64/bin/perry");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"").unwrap();
    exe
}

#[test]
fn package_roots_follow_the_install_layouts_npm_before_perry_home() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let home = root.join("home");
    let version = env!("CARGO_PKG_VERSION");
    let npm = package(
        &npm_dir(&modules, "wasm32-wasip1"),
        "wasm32-wasip1",
        version,
    );
    let mine = package(
        &perry_home_root(&home, PackageKind::Target, version, "wasm32-wasip1"),
        "wasm32-wasip1",
        version,
    );

    let candidates = collect_library_candidates_from(LIB, Some("wasip1"), Some(&exe), Some(&home));
    let pos = |p: &Path| candidates.iter().position(|c| c == p);
    // The Windows-zip layout `<bin>/<triple>/release` is an install layout.
    let zip = exe
        .parent()
        .unwrap()
        .join("wasm32-wasip1/release")
        .join(LIB);
    let (zip, npm, mine) = (
        pos(&zip).expect("zip layout probed"),
        pos(&npm.join("lib").join(LIB)).expect("npm root probed"),
        pos(&mine.join("lib").join(LIB)).expect("~/.perry root probed"),
    );
    assert!(zip < npm && npm < mine, "{candidates:#?}");
    assert_eq!(
        mine,
        candidates.len() - 1,
        "package roots are the last step"
    );
}

#[test]
fn a_root_for_another_version_adds_no_candidate() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let stale = package(
        &npm_dir(&modules, "wasm32-wasip2"),
        "wasm32-wasip2",
        "0.0.1",
    );
    let candidates = collect_library_candidates_from(LIB, Some("wasi"), Some(&exe), None);
    assert!(
        !candidates.iter().any(|c| c.starts_with(&stale)),
        "{candidates:#?}"
    );
}

#[test]
fn the_host_triple_never_resolves_through_a_package() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let host = package(
        &npm_dir(&modules, HOST_TRIPLE),
        HOST_TRIPLE,
        env!("CARGO_PKG_VERSION"),
    );
    for target in [None, Some("linux"), Some("macos"), Some("windows")] {
        let candidates = collect_library_candidates_from(LIB, target, Some(&exe), None);
        assert!(
            !candidates.iter().any(|c| c.starts_with(&host)),
            "{target:?}: {candidates:#?}"
        );
    }
}

#[test]
fn a_compressed_archive_in_a_package_links_the_cache_slot_and_keeps_the_sidecar_beside_it() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let pkg = package(
        &npm_dir(&modules, "aarch64-apple-ios"),
        "aarch64-apple-ios",
        env!("CARGO_PKG_VERSION"),
    );
    let archive = b"!<arch>\n";
    fs::write(
        pkg.join("lib").join(format!("{LIB}.zst")),
        zstd::encode_all(&archive[..], 3).unwrap(),
    )
    .unwrap();
    fs::write(
        pkg.join("lib").join(format!("{LIB}.gc-effects")),
        b"sidecar",
    )
    .unwrap();
    let cache = root.join("cache");
    // Run with RUST_TEST_THREADS=1 like the rest of the env-touching tests.
    std::env::set_var("PERRY_LIB_CACHE_DIR", &cache);

    let candidates = collect_library_candidates_from(LIB, Some("ios"), Some(&exe), None);
    let located = locate_among(candidates, LIB);
    std::env::remove_var("PERRY_LIB_CACHE_DIR");
    let located = located.expect("the package's runtime is found");

    assert_eq!(located.candidate, pkg.join("lib").join(LIB));
    assert!(located.link.starts_with(&cache), "{:?}", located.link);
    assert_eq!(fs::read(&located.link).unwrap(), archive);
    let sidecar = perry_gc_effects::sidecar::sidecar_path(&located.candidate);
    assert_eq!(fs::read(sidecar).unwrap(), b"sidecar");
}

const EXT: &str = "libperry_ext_http.a";

#[test]
fn ext_archives_come_from_the_host_ext_package_after_the_install_layouts() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let home = root.join("home");
    let version = env!("CARGO_PKG_VERSION");
    let npm = package(&npm_ext_dir(&modules, HOST_TRIPLE), HOST_TRIPLE, version);
    let mine = package(
        &perry_home_root(&home, PackageKind::Ext, version, HOST_TRIPLE),
        HOST_TRIPLE,
        version,
    );
    let candidates = collect_library_candidates_from(EXT, None, Some(&exe), Some(&home));
    let pos = |p: &Path| candidates.iter().position(|c| c == p);
    let beside = pos(&exe.parent().unwrap().join(EXT)).expect("exe dir probed");
    let npm = pos(&npm.join("lib").join(EXT)).expect("npm ext package probed");
    let mine = pos(&mine.join("lib").join(EXT)).expect("~/.perry/ext probed");
    assert!(beside < npm && npm < mine, "{candidates:#?}");
    assert_eq!(
        mine,
        candidates.len() - 1,
        "package roots are the last step"
    );

    // A core archive never comes from the ext package, an ext archive never
    // from a target package.
    let core = collect_library_candidates_from(LIB, None, Some(&exe), Some(&home));
    assert!(
        !core.iter().any(|c| c.starts_with(root.join("home"))),
        "{core:#?}"
    );
    let target = package(
        &npm_dir(&modules, "wasm32-wasip1"),
        "wasm32-wasip1",
        version,
    );
    let cross = collect_library_candidates_from(EXT, Some("wasip1"), Some(&exe), Some(&home));
    assert!(!cross.iter().any(|c| c.starts_with(&target)), "{cross:#?}");
}

#[test]
fn an_ext_package_of_another_version_adds_no_candidate() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let stale = package(&npm_ext_dir(&modules, HOST_TRIPLE), HOST_TRIPLE, "0.0.1");
    let other = package(
        &npm_ext_dir(&modules, "aarch64-linux-android"),
        "aarch64-linux-android",
        env!("CARGO_PKG_VERSION"),
    );
    let candidates = collect_library_candidates_from(EXT, None, Some(&exe), None);
    assert!(
        !candidates
            .iter()
            .any(|c| c.starts_with(&stale) || c.starts_with(&other)),
        "{candidates:#?}"
    );
}
