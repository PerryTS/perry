use super::*;

/// #7137 follow-up. Compile-package dedup keeps one directory per package
/// name and routes every importer to it. That is right for a genuine
/// duplicate install and wrong-but-silent when the two copies are different
/// versions — which auto-compile made reachable for a project's entire
/// dependency graph rather than only for hand-listed packages.
///
/// `dedup_collapses_distinct_versions` is the predicate behind the warning,
/// so it has to be true exactly in the lossy case.
fn write_pkg(dir: &std::path::Path, version: &str) {
    std::fs::create_dir_all(dir).expect("mkdir");
    std::fs::write(
        dir.join("package.json"),
        format!(r#"{{"name":"dup-pkg","version":"{version}"}}"#),
    )
    .expect("write package.json");
}

#[test]
fn differing_versions_are_reported_as_collapsing() {
    let root = tempfile::tempdir().expect("tempdir");
    let chosen = root.path().join("node_modules/dup-pkg");
    let found = root.path().join("sub/node_modules/dup-pkg");
    write_pkg(&chosen, "1.0.0");
    write_pkg(&found, "2.0.0");

    assert!(
        dedup_collapses_distinct_versions(&chosen, &found),
        "1.0.0 substituted for 2.0.0 is a silent loss and must be reported"
    );
}

#[test]
fn identical_versions_are_a_plain_duplicate_install() {
    let root = tempfile::tempdir().expect("tempdir");
    let chosen = root.path().join("node_modules/dup-pkg");
    let found = root.path().join("sub/node_modules/dup-pkg");
    write_pkg(&chosen, "1.0.0");
    write_pkg(&found, "1.0.0");

    assert!(
        !dedup_collapses_distinct_versions(&chosen, &found),
        "collapsing two copies of the same version is the intended dedup"
    );
}

#[test]
fn same_directory_is_never_a_collapse() {
    let root = tempfile::tempdir().expect("tempdir");
    let only = root.path().join("node_modules/dup-pkg");
    write_pkg(&only, "1.0.0");

    assert!(
        !dedup_collapses_distinct_versions(&only, &only),
        "the first copy resolving to itself is not a substitution"
    );
}

/// A copy with no readable `package.json` cannot be proven distinct — a
/// local symlinked workspace package often has no version at the resolved
/// path. Warning there would be noise on every such build.
#[test]
fn unreadable_version_stays_quiet() {
    let root = tempfile::tempdir().expect("tempdir");
    let chosen = root.path().join("node_modules/dup-pkg");
    let found = root.path().join("linked/dup-pkg");
    write_pkg(&chosen, "1.0.0");
    std::fs::create_dir_all(&found).expect("mkdir");

    assert!(
        !dedup_collapses_distinct_versions(&chosen, &found),
        "an unversioned copy must not produce a warning"
    );
}

/// The shadowing is not (only) the `compile_package_dirs` dedup: for a
/// package in `compile_packages`, `resolve_import` searches the PROJECT ROOT
/// before the importer's own ancestors. So a nested copy is passed over even
/// on its first resolution. `node_nearest_package_dir` is what Node would
/// have picked, and is what the warning compares against.
#[test]
fn nearest_dir_is_the_importers_own_node_modules() {
    let root = tempfile::tempdir().expect("tempdir");
    let top = root.path().join("node_modules/dup-pkg");
    let nested = root.path().join("sub/node_modules/dup-pkg");
    write_pkg(&top, "1.0.0");
    write_pkg(&nested, "2.0.0");
    let importer = root.path().join("sub/child.ts");
    std::fs::write(&importer, "export {};\n").expect("write importer");

    let nearest = node_nearest_package_dir("dup-pkg", &importer).expect("nearest copy found");
    assert_eq!(
        nearest, nested,
        "Node resolves a bare specifier from the importer's nearest node_modules"
    );
    assert!(
        dedup_collapses_distinct_versions(&top, &nearest),
        "compiling the root 1.0.0 for an importer Node would give 2.0.0 is a \
         silent version substitution"
    );
}

/// An importer with no nearer copy resolves to the same directory Perry
/// chose — nothing was shadowed, so nothing is reported.
#[test]
fn importer_without_a_nearer_copy_is_not_shadowed() {
    let root = tempfile::tempdir().expect("tempdir");
    let top = root.path().join("node_modules/dup-pkg");
    write_pkg(&top, "1.0.0");
    let importer = root.path().join("sub/child.ts");
    std::fs::create_dir_all(importer.parent().unwrap()).expect("mkdir");
    std::fs::write(&importer, "export {};\n").expect("write importer");

    let nearest = node_nearest_package_dir("dup-pkg", &importer).expect("root copy found");
    assert_eq!(nearest, top);
    assert!(!dedup_collapses_distinct_versions(&top, &nearest));
}
