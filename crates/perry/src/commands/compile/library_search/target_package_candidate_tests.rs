//! Search step 4: target and ext packages come after every install layout,
//! and only a root whose target.json matches this perry adds candidates.

use super::super::target_packages::{
    package_name, perry_home_root, PackageKind, PerryIdentity, TargetFile, TargetJson, HOST_TRIPLE,
    TARGET_JSON,
};
use super::{
    collect_library_candidates_from, locate_among, locate_or_fetch_from, package_kind_for,
};
use crate::commands::target::PackageFetch;
use std::cell::RefCell;
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
    // Locate only the package's own candidate: the ambient candidates (the
    // PERRY_RUNTIME_DIR / PERRY_LIB_DIR overrides, a checkout's
    // `target/<triple>/release`) are probed first and win whenever they exist,
    // as they do in CI where PERRY_RUNTIME_DIR names the release dir.
    let candidates: Vec<PathBuf> =
        collect_library_candidates_from(LIB, Some("ios"), Some(&exe), None)
            .into_iter()
            .filter(|c| c.starts_with(&pkg))
            .collect();
    assert_eq!(candidates, [pkg.join("lib").join(LIB)]);
    // Run with RUST_TEST_THREADS=1 like the rest of the env-touching tests.
    std::env::set_var("PERRY_LIB_CACHE_DIR", &cache);
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

// --- locate_or_fetch ----------------------------------------------------------

/// A fetcher that records its calls and, when `install` is set, puts a package
/// with `LIB` in the ~/.perry root it was asked for.
fn fetcher<'a>(
    calls: &'a RefCell<Vec<(PackageKind, String)>>,
    home: &'a Path,
    install: bool,
) -> impl FnOnce(PackageKind, &str) -> Option<PackageFetch> + 'a {
    move |kind, triple| {
        calls.borrow_mut().push((kind, triple.to_string()));
        if !install {
            return Some(PackageFetch::Refused("not today".into()));
        }
        let dest = package(
            &perry_home_root(home, kind, env!("CARGO_PKG_VERSION"), triple),
            triple,
            env!("CARGO_PKG_VERSION"),
        );
        fs::write(dest.join("lib").join(LIB), b"archive").unwrap();
        Some(PackageFetch::Installed)
    }
}

#[test]
fn a_cross_runtime_miss_fetches_its_target_package_once_and_finds_it() {
    let (_t, root) = tree();
    let home = root.join("home");
    let calls = RefCell::new(Vec::new());
    let found = locate_or_fetch_from(
        LIB,
        Some("wasip1"),
        None,
        Some(&home),
        fetcher(&calls, &home, true),
    )
    .unwrap_or_else(|m| panic!("not found after the fetch: {:#?}", m.searched));
    assert_eq!(
        *calls.borrow(),
        [(PackageKind::Target, "wasm32-wasip1".to_string())]
    );
    assert!(
        found.link.starts_with(root.join("home/.perry/targets")),
        "{:?}",
        found.link
    );
}

#[test]
fn a_refused_fetch_keeps_the_search_list_and_carries_the_reason() {
    let (_t, root) = tree();
    let home = root.join("home");
    let calls = RefCell::new(Vec::new());
    let Err(missed) = locate_or_fetch_from(
        LIB,
        Some("ios"),
        None,
        Some(&home),
        fetcher(&calls, &home, false),
    ) else {
        panic!("a refused fetch finds nothing");
    };
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(missed.refused.as_deref(), Some("not today"));
    assert!(!missed.searched.is_empty());
}

#[test]
fn nothing_is_fetched_for_the_host_an_instrumented_archive_or_an_installed_package() {
    let (_t, root) = tree();
    let home = root.join("home");
    let calls = RefCell::new(Vec::new());
    // The host's own runtime ships with perry: never a package.
    let host = locate_or_fetch_from(LIB, None, None, Some(&home), fetcher(&calls, &home, true));
    assert!(host.is_err());
    // The instrumented variants are not in any package.
    for name in ["libperry_runtime_instr.a", "perry_stdlib_instr.lib"] {
        let r = locate_or_fetch_from(
            name,
            Some("wasip1"),
            None,
            Some(&home),
            fetcher(&calls, &home, true),
        );
        assert!(r.is_err(), "{name}");
    }
    // A package that is installed but lacks the archive is not fetched again.
    package(
        &perry_home_root(
            &home,
            PackageKind::Target,
            env!("CARGO_PKG_VERSION"),
            "wasm32-wasip1",
        ),
        "wasm32-wasip1",
        env!("CARGO_PKG_VERSION"),
    );
    let r = locate_or_fetch_from(
        LIB,
        Some("wasip1"),
        None,
        Some(&home),
        fetcher(&calls, &home, true),
    );
    assert!(r.is_err());
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
}

#[test]
fn the_package_of_an_archive_depends_on_its_name_and_triple() {
    assert_eq!(
        package_kind_for(LIB, "wasm32-wasip1"),
        Some(PackageKind::Target)
    );
    assert_eq!(package_kind_for(LIB, HOST_TRIPLE), None);
    assert_eq!(
        package_kind_for("libperry_ext_http.a", HOST_TRIPLE),
        Some(PackageKind::Ext)
    );
    assert_eq!(
        package_kind_for("perry_ext_http.lib", "wasm32-wasip1"),
        Some(PackageKind::Ext)
    );
}

// --- on-demand host kinds ---------------------------------------------------

/// The UI archive `find_ui_library` asks for on this host.
const HOST_UI: &str = if cfg!(target_os = "windows") {
    "perry_ui_windows.lib"
} else if cfg!(target_os = "linux") {
    "libperry_ui_gtk4.a"
} else {
    "libperry_ui_macos.a"
};
const HOST_WASM: &str = if cfg!(target_os = "windows") {
    "perry_wasm_host.lib"
} else {
    "libperry_wasm_host.a"
};

#[test]
fn the_host_ui_and_wasm_host_archives_ship_in_separate_host_packages() {
    for (name, kind) in [
        ("libperry_ui_gtk4.a", PackageKind::Ui),
        ("libperry_ui_macos.a", PackageKind::Ui),
        ("perry_ui_windows.lib", PackageKind::Ui),
        ("libperry_wasm_host.a", PackageKind::WasmHost),
        ("perry_wasm_host.lib", PackageKind::WasmHost),
        ("libperry_ext_bcrypt.a", PackageKind::Ext),
        ("perry_ext_http.lib", PackageKind::Ext),
    ] {
        assert_eq!(package_kind_for(name, HOST_TRIPLE), Some(kind), "{name}");
    }
    // A cross target's UI and wasm_host come with its target package.
    assert_eq!(
        package_kind_for("libperry_ui_android.a", "aarch64-linux-android"),
        Some(PackageKind::Target)
    );
    assert_eq!(
        package_kind_for("libperry_wasm_host.a", "wasm32-wasip1"),
        Some(PackageKind::Target)
    );
    // Everything a plain program links ships with perry.
    for name in [
        "libperry_runtime.a",
        "libperry_stdlib.a",
        "perry_stdlib.lib",
        "libperry_runtime.a.gc-effects",
    ] {
        assert_eq!(package_kind_for(name, HOST_TRIPLE), None, "{name}");
    }
}

#[test]
fn the_host_ui_and_wasm_host_come_from_separate_packages_after_the_install_layouts() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = exe_in(&modules);
    let home = root.join("home");
    let version = env!("CARGO_PKG_VERSION");
    for (name, kind) in [
        (HOST_UI, PackageKind::Ui),
        (HOST_WASM, PackageKind::WasmHost),
    ] {
        let npm = package(
            &modules.join(package_name(kind, HOST_TRIPLE)),
            HOST_TRIPLE,
            version,
        );
        let mine = package(
            &perry_home_root(&home, kind, version, HOST_TRIPLE),
            HOST_TRIPLE,
            version,
        );
        let candidates = collect_library_candidates_from(name, None, Some(&exe), Some(&home));
        let pos = |p: &Path| candidates.iter().position(|c| c == p);
        let beside = pos(&exe.parent().unwrap().join(name)).expect("exe dir probed");
        let in_npm = pos(&npm.join("lib").join(name)).expect("npm ext package probed");
        let in_home = pos(&mine.join("lib").join(name)).expect("~/.perry/ext probed");
        assert!(
            beside < in_npm && in_npm < in_home,
            "{name}: {candidates:#?}"
        );
        assert_eq!(
            in_home,
            candidates.len() - 1,
            "{name}: package roots are the last step"
        );
    }
}

/// A fetcher that records its calls and installs a ~/.perry package holding
/// `archive` for the kind and triple it is asked for.
fn installing<'a>(
    calls: &'a RefCell<Vec<(PackageKind, String)>>,
    home: &'a Path,
    archive: &'a str,
) -> impl FnOnce(PackageKind, &str) -> Option<PackageFetch> + 'a {
    move |kind, triple| {
        calls.borrow_mut().push((kind, triple.to_string()));
        let dest = package(
            &perry_home_root(home, kind, env!("CARGO_PKG_VERSION"), triple),
            triple,
            env!("CARGO_PKG_VERSION"),
        );
        fs::write(dest.join("lib").join(archive), b"archive").unwrap();
        Some(PackageFetch::Installed)
    }
}

#[test]
fn a_host_ui_or_wasm_host_miss_fetches_only_its_kind_once() {
    for (name, kind) in [
        (HOST_UI, PackageKind::Ui),
        (HOST_WASM, PackageKind::WasmHost),
    ] {
        let (_t, root) = tree();
        let modules = root.join("prefix/lib/node_modules");
        let exe = exe_in(&modules);
        let home = root.join("home");
        // A workspace or system install that has the archive answers first
        // and nothing is fetched; this test needs the npm layout's miss.
        if let Ok(found) = locate_among(
            collect_library_candidates_from(name, None, Some(&exe), Some(&home)),
            name,
        ) {
            eprintln!("skip {name}: an install layout has it at {:?}", found.link);
            continue;
        }
        let calls = RefCell::new(Vec::new());
        let found = locate_or_fetch_from(
            name,
            None,
            Some(&exe),
            Some(&home),
            installing(&calls, &home, name),
        )
        .unwrap_or_else(|m| panic!("{name} not found after the fetch: {:#?}", m.searched));
        assert_eq!(*calls.borrow(), [(kind, HOST_TRIPLE.to_string())], "{name}");
        assert!(
            found
                .link
                .starts_with(root.join("home/.perry").join(kind.home_dir())),
            "{name}: {:?}",
            found.link
        );
        // The next lookup finds the installed package and fetches nothing.
        let again = locate_or_fetch_from(name, None, Some(&exe), Some(&home), |_, _| {
            panic!("{name}: fetched again")
        });
        assert_eq!(again.map(|l| l.link).ok(), Some(found.link), "{name}");
    }
}
