use super::*;

fn write_pkg(dir: &Path, version: &str, marker: &str) {
    std::fs::create_dir_all(dir).expect("mkdir package");
    std::fs::write(
        dir.join("package.json"),
        format!(r#"{{"name":"dup-pkg","version":"{version}","main":"index.js"}}"#),
    )
    .expect("write package.json");
    std::fs::write(
        dir.join("index.js"),
        format!("export default '{marker}';\n"),
    )
    .expect("write package entry");
}

fn two_version_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let top = root.join("node_modules/dup-pkg");
    let holder = root.join("node_modules/holder");
    let nested = holder.join("node_modules/dup-pkg");
    write_pkg(&top, "1.0.0", "top-v1");
    write_pkg(&nested, "2.0.0", "nested-v2");
    std::fs::create_dir_all(&holder).expect("mkdir holder");

    let root_importer = root.join("main.ts");
    let nested_importer = holder.join("index.js");
    std::fs::write(&root_importer, "export {};\n").expect("write root importer");
    std::fs::write(&nested_importer, "export {};\n").expect("write nested importer");
    (top, nested, root_importer, nested_importer)
}

#[test]
fn compile_package_resolution_preserves_importer_relative_instances() {
    let fixture = tempfile::tempdir().expect("tempdir");
    let root = fixture.path();
    let (top, nested, root_importer, nested_importer) = two_version_fixture(root);
    let compile_packages = HashSet::from(["dup-pkg".to_string()]);

    // Model the real collection order: the top-level copy has already been
    // discovered and recorded before the nested importer is resolved.
    let mut package_roots = BTreeSet::new();
    package_roots.insert(top.canonicalize().expect("canonical top package"));

    let (top_entry, top_kind) = resolve_import(
        "dup-pkg",
        &root_importer,
        root,
        &compile_packages,
        &package_roots,
    )
    .expect("resolve top-level package");
    let (nested_entry, nested_kind) = resolve_import(
        "dup-pkg",
        &nested_importer,
        root,
        &compile_packages,
        &package_roots,
    )
    .expect("resolve nested package");

    assert_eq!(top_kind, ModuleKind::NativeCompiled);
    assert_eq!(nested_kind, ModuleKind::NativeCompiled);
    assert_eq!(top_entry, top.join("index.js").canonicalize().unwrap());
    assert_eq!(
        nested_entry,
        nested.join("index.js").canonicalize().unwrap(),
        "a previously discovered copy with the same package name must not redirect this importer"
    );
    assert_ne!(top_entry, nested_entry);
}

#[test]
fn resolve_cache_keys_the_same_specifier_by_importer_directory() {
    let fixture = tempfile::tempdir().expect("tempdir");
    let root = fixture.path();
    let (top, nested, root_importer, nested_importer) = two_version_fixture(root);
    let mut ctx = CompilationContext::new(root.to_path_buf());
    ctx.compile_packages.insert("dup-pkg".to_string());
    ctx.compile_package_dirs
        .insert(top.canonicalize().expect("canonical top package"));

    let (top_entry, _) =
        cached_resolve_import("dup-pkg", &root_importer, &mut ctx).expect("top resolution");
    let (nested_entry, _) =
        cached_resolve_import("dup-pkg", &nested_importer, &mut ctx).expect("nested resolution");

    assert_eq!(top_entry, top.join("index.js").canonicalize().unwrap());
    assert_eq!(
        nested_entry,
        nested.join("index.js").canonicalize().unwrap()
    );
    assert_eq!(ctx.resolve_cache.len(), 2);
}

#[test]
fn package_root_identity_deduplicates_only_the_same_canonical_instance() {
    let fixture = tempfile::tempdir().expect("tempdir");
    let root = fixture.path();
    let (top, nested, _, _) = two_version_fixture(root);
    let top = top.canonicalize().unwrap();
    let nested = nested.canonicalize().unwrap();
    let mut roots = BTreeSet::new();

    assert!(roots.insert(top.clone()));
    assert!(!roots.insert(top));
    assert!(roots.insert(nested));
    assert_eq!(roots.len(), 2);
}
