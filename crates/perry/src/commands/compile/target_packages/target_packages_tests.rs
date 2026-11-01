use super::*;
use std::path::{Path, PathBuf};

const TRIPLE: &str = "aarch64-linux-android";
const ME: PerryIdentity<'static> = PerryIdentity {
    version: "0.5.1700",
    revision: Some("1111111111111111111111111111111111111111"),
};

/// A temp tree, canonicalized so the expected paths match `real_exe`'s.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    (dir, root)
}

fn touch(path: &Path) -> PathBuf {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"").unwrap();
    path.to_path_buf()
}

fn sha(bytes: &[u8]) -> String {
    perry_hex::encode(Sha256::digest(bytes))
}

/// A package root for `triple` declaring `version`/`revision`, with one archive.
fn package(root: &Path, triple: &str, version: &str, revision: &str) -> PathBuf {
    fs::create_dir_all(root.join("lib")).unwrap();
    let archive = b"!<arch>\nruntime";
    fs::write(root.join("lib/libperry_runtime.a"), archive).unwrap();
    let manifest = TargetJson {
        schema_version: 1,
        perry_version: version.into(),
        source_revision: revision.into(),
        target_triple: triple.into(),
        files: vec![TargetFile {
            path: "lib/libperry_runtime.a".into(),
            sha256: sha(archive),
            size: archive.len() as u64,
        }],
    };
    fs::write(
        root.join(TARGET_JSON),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    root.to_path_buf()
}

fn good(root: &Path) -> PathBuf {
    package(root, TRIPLE, ME.version, ME.revision.unwrap())
}

fn target_dir(node_modules: &Path) -> PathBuf {
    node_modules
        .join("@perryts")
        .join(format!("perry-target-{TRIPLE}"))
}

fn accepted(exe: Option<&Path>, home: Option<&Path>) -> Vec<(PathBuf, RootSource)> {
    discover(PackageKind::Target, TRIPLE, exe, home, ME)
        .into_iter()
        .filter(TargetRoot::is_accepted)
        .map(|r| (r.path, r.source))
        .collect()
}

#[test]
fn the_package_name_is_derived_from_the_triple() {
    assert_eq!(
        package_name(PackageKind::Target, "wasm32-wasip1"),
        "@perryts/perry-target-wasm32-wasip1"
    );
    assert_eq!(
        package_name(PackageKind::Target, TRIPLE),
        "@perryts/perry-target-aarch64-linux-android"
    );
}

#[test]
fn global_unix_install_finds_the_package_in_the_prefix() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe =
        touch(&modules.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin/perry"));
    touch(&modules.join("@perryts/perry/package.json"));
    let pkg = good(&target_dir(&modules));
    assert_eq!(accepted(Some(&exe), None), [(pkg, RootSource::Npm)]);
    assert_eq!(npm_install(&exe), Some(NpmInstall::Global));
}

#[test]
fn global_windows_install_finds_the_package_beside_npm() {
    let (_t, root) = tree();
    let modules = root.join("AppData/Roaming/npm/node_modules");
    let exe =
        touch(&modules.join("@perryts/perry/node_modules/@perryts/perry-win32-x64/bin/perry.exe"));
    touch(&modules.join("@perryts/perry/package.json"));
    let pkg = good(&target_dir(&modules));
    assert_eq!(accepted(Some(&exe), None), [(pkg, RootSource::Npm)]);
    assert_eq!(npm_install(&exe), Some(NpmInstall::Global));
}

#[test]
fn project_install_hoisted_and_nested() {
    let (_t, root) = tree();
    let project = root.join("app");
    touch(&project.join("package.json"));
    let modules = project.join("node_modules");
    touch(&modules.join("@perryts/perry/package.json"));

    let hoisted = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let pkg = good(&target_dir(&modules));
    assert_eq!(
        accepted(Some(&hoisted), None),
        [(pkg.clone(), RootSource::Npm)]
    );
    assert_eq!(
        npm_install(&hoisted),
        Some(NpmInstall::Project(project.clone()))
    );

    // Nested under the wrapper: a nearer package wins over the hoisted one.
    let nested_modules = modules.join("@perryts/perry/node_modules");
    let nested = touch(&nested_modules.join("@perryts/perry-linux-x64/bin/perry"));
    let near = good(&target_dir(&nested_modules));
    assert_eq!(
        accepted(Some(&nested), None),
        [(near, RootSource::Npm), (pkg, RootSource::Npm)]
    );
    assert_eq!(npm_install(&nested), Some(NpmInstall::Project(project)));
}

#[cfg(unix)]
#[test]
fn pnpm_resolves_from_the_real_binary_path() {
    let (_t, root) = tree();
    let project = root.join("app");
    touch(&project.join("package.json"));
    let modules = project.join("node_modules");
    let store = modules.join(".pnpm");
    let real_host =
        store.join("@perryts+perry-linux-x64@0.5.1700/node_modules/@perryts/perry-linux-x64");
    touch(&real_host.join("bin/perry"));
    let real_wrapper = store.join("@perryts+perry@0.5.1700/node_modules/@perryts/perry");
    touch(&real_wrapper.join("package.json"));
    let real_target = good(&target_dir(&store.join(format!(
        "@perryts+perry-target-{TRIPLE}@0.5.1700/node_modules"
    ))));
    fs::create_dir_all(modules.join("@perryts")).unwrap();
    std::os::unix::fs::symlink(&real_wrapper, modules.join("@perryts/perry")).unwrap();
    std::os::unix::fs::symlink(&real_target, target_dir(&modules)).unwrap();
    // The binary as a symlinked path: perry resolves it first.
    let link = root.join("bin-link");
    std::os::unix::fs::symlink(real_host.join("bin/perry"), &link).unwrap();

    assert_eq!(
        accepted(Some(&link), None),
        [(target_dir(&modules), RootSource::Npm)]
    );
    assert_eq!(npm_install(&link), Some(NpmInstall::Project(project)));
}

#[test]
fn perry_home_comes_after_npm_and_is_keyed_by_version() {
    let (_t, root) = tree();
    let home = root.join("home");
    let modules = root.join("prefix/lib/node_modules");
    let exe = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let npm = good(&target_dir(&modules));
    let mine = good(&perry_home_root(
        &home,
        PackageKind::Target,
        ME.version,
        TRIPLE,
    ));
    package(
        &perry_home_root(&home, PackageKind::Target, "0.5.1600", TRIPLE),
        TRIPLE,
        "0.5.1600",
        ME.revision.unwrap(),
    );
    assert_eq!(
        accepted(Some(&exe), Some(&home)),
        [
            (npm, RootSource::Npm),
            (mine.clone(), RootSource::PerryHome)
        ]
    );
    assert_eq!(accepted(None, Some(&home)), [(mine, RootSource::PerryHome)]);
}

#[test]
fn a_mismatched_root_is_skipped_and_a_later_valid_one_used() {
    let (_t, root) = tree();
    let home = root.join("home");
    let modules = root.join("prefix/lib/node_modules");
    let exe = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let old = package(
        &target_dir(&modules),
        TRIPLE,
        "0.5.1690",
        ME.revision.unwrap(),
    );
    let valid = good(&perry_home_root(
        &home,
        PackageKind::Target,
        ME.version,
        TRIPLE,
    ));

    let roots = discover(PackageKind::Target, TRIPLE, Some(&exe), Some(&home), ME);
    assert_eq!(roots.len(), 2);
    let rejected = roots[0].status.as_ref().unwrap_err();
    assert_eq!(roots[0].path, old);
    assert_eq!(rejected.found_version.as_deref(), Some("0.5.1690"));
    assert!(
        rejected.reason.contains("perry is 0.5.1700"),
        "{}",
        rejected.reason
    );
    assert_eq!(roots[1].path, valid);
    assert!(roots[1].is_accepted());
}

#[test]
fn triple_revision_and_schema_mismatches_are_rejected() {
    let (_t, root) = tree();
    let other_triple = package(
        &root.join("a"),
        "x86_64-linux-android",
        ME.version,
        ME.revision.unwrap(),
    );
    assert!(check_root(&other_triple, TRIPLE, ME)
        .unwrap_err()
        .reason
        .contains("holds x86_64-linux-android"));

    let other_rev = package(&root.join("b"), TRIPLE, ME.version, &"2".repeat(40));
    let reason = check_root(&other_rev, TRIPLE, ME).unwrap_err().reason;
    assert!(reason.contains("built from 222222222222"), "{reason}");
    // A local build (no compiled revision) checks the version only.
    let local = PerryIdentity {
        version: ME.version,
        revision: None,
    };
    assert!(check_root(&other_rev, TRIPLE, local).is_ok());

    let unreadable = root.join("c");
    fs::create_dir_all(&unreadable).unwrap();
    fs::write(unreadable.join(TARGET_JSON), b"{").unwrap();
    let rejected = check_root(&unreadable, TRIPLE, ME).unwrap_err();
    assert_eq!(rejected.found_version, None);

    let schema = good(&root.join("d"));
    let text = fs::read_to_string(schema.join(TARGET_JSON))
        .unwrap()
        .replace("\"schema_version\": 1", "\"schema_version\": 2");
    fs::write(schema.join(TARGET_JSON), text).unwrap();
    assert!(check_root(&schema, TRIPLE, ME).is_err());
}

#[test]
fn missing_target_error_global_form_pins_the_exact_version() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    touch(&modules.join("@perryts/perry/package.json"));
    let exe =
        touch(&modules.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin/perry"));
    let message = missing_target_message("android", TRIPLE, Some(&exe), None, ME);
    assert_eq!(
        message,
        "perry 0.5.1700 has no libraries for target `android` (aarch64-linux-android).\n\
         Install them:\n  \
         npm install -g @perryts/perry-target-aarch64-linux-android@0.5.1700\n\
         or, without npm:\n  \
         perry target add android\n"
    );
}

#[test]
fn missing_target_error_project_form_names_the_project() {
    let (_t, root) = tree();
    let project = root.join("app");
    touch(&project.join("package.json"));
    let modules = project.join("node_modules");
    touch(&modules.join("@perryts/perry/package.json"));
    let exe = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let message = missing_target_message("android", TRIPLE, Some(&exe), None, ME);
    assert!(message.contains(&format!(
        "  npm install --save-dev @perryts/perry-target-aarch64-linux-android@0.5.1700   (in {})\n",
        project.display()
    )));
    assert!(!message.contains(" -g "));
}

#[test]
fn missing_target_error_without_npm_offers_target_add_only() {
    let (_t, root) = tree();
    let exe = touch(&root.join("opt/homebrew/bin/perry"));
    let message = missing_target_message("wasi", "wasm32-wasip2", Some(&exe), None, ME);
    assert_eq!(
        message,
        "perry 0.5.1700 has no libraries for target `wasi` (wasm32-wasip2).\n\
         Install them:\n  \
         perry target add wasi\n"
    );
}

#[test]
fn missing_target_error_names_a_rejected_root() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let old = package(
        &target_dir(&modules),
        TRIPLE,
        "0.5.1690",
        ME.revision.unwrap(),
    );
    let message = missing_target_message("android", TRIPLE, Some(&exe), None, ME);
    assert!(
        message.contains(&format!(
            "found @perryts/perry-target-aarch64-linux-android 0.5.1690 at {}, not used: it is 0.5.1690; perry is 0.5.1700\n",
            old.display()
        )),
        "{message}"
    );
}

#[test]
fn verify_files_hashes_raw_and_compressed_archives() {
    let (_t, root) = tree();
    let pkg = good(&root.join("pkg"));
    let manifest = check_root(&pkg, TRIPLE, ME).unwrap();
    verify_files(&pkg, &manifest).unwrap();

    // The npm layout: compressed, hashed after decompression.
    let raw = fs::read(pkg.join("lib/libperry_runtime.a")).unwrap();
    fs::write(
        pkg.join("lib/libperry_runtime.a.zst"),
        zstd::encode_all(&raw[..], 3).unwrap(),
    )
    .unwrap();
    fs::remove_file(pkg.join("lib/libperry_runtime.a")).unwrap();
    verify_files(&pkg, &manifest).unwrap();

    // An unlisted file is refused.
    fs::write(pkg.join("lib/extra.a"), b"x").unwrap();
    assert!(verify_files(&pkg, &manifest).is_err());
    fs::remove_file(pkg.join("lib/extra.a")).unwrap();

    // Different bytes are refused.
    fs::write(
        pkg.join("lib/libperry_runtime.a.zst"),
        zstd::encode_all(&b"other"[..], 3).unwrap(),
    )
    .unwrap();
    assert!(verify_files(&pkg, &manifest).is_err());

    // A path outside lib/ is refused before anything is read.
    let mut escaping = manifest.clone();
    escaping.files[0].path = "lib/../../etc/passwd".into();
    assert!(verify_files(&pkg, &escaping).is_err());
}

#[test]
fn ext_packages_share_the_layout_under_their_own_names() {
    assert_eq!(
        package_name(PackageKind::Ext, "x86_64-unknown-linux-gnu"),
        "@perryts/perry-ext-x86_64-unknown-linux-gnu"
    );
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe =
        touch(&modules.join("@perryts/perry/node_modules/@perryts/perry-linux-x64/bin/perry"));
    let home = root.join("home");
    // A target package of the same triple is not an ext package, and back.
    good(&target_dir(&modules));
    assert!(discover(PackageKind::Ext, TRIPLE, Some(&exe), Some(&home), ME).is_empty());
    let npm = good(&modules.join("@perryts").join(format!("perry-ext-{TRIPLE}")));
    let mine = good(&perry_home_root(
        &home,
        PackageKind::Ext,
        ME.version,
        TRIPLE,
    ));
    assert!(mine.starts_with(home.join(".perry/ext")));
    let roots: Vec<_> = discover(PackageKind::Ext, TRIPLE, Some(&exe), Some(&home), ME)
        .into_iter()
        .map(|r| (r.path, r.source, r.status.is_ok()))
        .collect();
    assert_eq!(
        roots,
        [
            (npm, RootSource::Npm, true),
            (mine, RootSource::PerryHome, true)
        ]
    );
}

#[test]
fn the_packaged_triples_come_from_the_one_list() {
    let list: serde_json::Value = serde_json::from_str(PACKAGED_TRIPLES_JSON).unwrap();
    for (kind, key) in [(PackageKind::Target, "targets"), (PackageKind::Ext, "ext")] {
        let triples = list[key].as_array().unwrap();
        assert!(!triples.is_empty(), "{key}");
        for triple in triples {
            assert!(
                is_packaged(kind, triple.as_str().unwrap()),
                "{key}: {triple}"
            );
        }
    }
    assert!(is_packaged(PackageKind::Target, "aarch64-linux-android"));
    assert!(is_packaged(PackageKind::Ext, "x86_64-unknown-linux-gnu"));
    // No package: windows-msvc cross targets, musl and Windows extensions.
    assert!(!is_packaged(PackageKind::Target, "x86_64-pc-windows-msvc"));
    assert!(!is_packaged(PackageKind::Ext, "x86_64-unknown-linux-musl"));
    assert!(!is_packaged(PackageKind::Ext, "x86_64-pc-windows-msvc"));
    assert!(!is_packaged(PackageKind::Ext, "aarch64-linux-android"));
}

#[test]
fn the_ext_install_hint_names_npm_and_perry_and_a_rejected_root() {
    let (_t, root) = tree();
    let modules = root.join("prefix/lib/node_modules");
    let exe = touch(&modules.join("@perryts/perry-linux-x64/bin/perry"));
    let triple = "x86_64-unknown-linux-gnu";
    package(
        &modules.join("@perryts").join(format!("perry-ext-{triple}")),
        triple,
        "0.5.1600",
        ME.revision.unwrap(),
    );
    let hint = install_hint(
        PackageKind::Ext,
        triple,
        "perry target add --ext",
        Some(&exe),
        None,
        ME,
    );
    assert!(
        hint.contains("npm install -g @perryts/perry-ext-x86_64-unknown-linux-gnu@0.5.1700\n"),
        "{hint}"
    );
    assert!(hint.contains("  perry target add --ext\n"), "{hint}");
    assert!(
        hint.contains("found @perryts/perry-ext-x86_64-unknown-linux-gnu 0.5.1600"),
        "{hint}"
    );
}
