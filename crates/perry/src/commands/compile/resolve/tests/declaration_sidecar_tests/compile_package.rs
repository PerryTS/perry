use super::*;

#[test]
fn subpath_exports_do_not_fall_back_to_src_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package_dir = root.join("node_modules").join("pkg");
    std::fs::create_dir_all(package_dir.join("src/feature")).expect("mkdir package");
    std::fs::write(
        package_dir.join("package.json"),
        r#"{
          "name": "pkg",
          "type": "module",
          "exports": {
            ".": { "import": { "default": "./src/index.ts" } },
            "./feature": { "import": { "default": "./src/feature/server.ts" } }
          }
        }"#,
    )
    .expect("write package.json");
    std::fs::write(
        package_dir.join("src/index.ts"),
        "export const rootOnly = 1;\n",
    )
    .expect("write root");
    std::fs::write(
        package_dir.join("src/feature/server.ts"),
        "export const subValue = 41;\n",
    )
    .expect("write subpath");

    let importer_dir = root.join("src");
    std::fs::create_dir_all(&importer_dir).expect("mkdir src");
    let importer = importer_dir.join("main.ts");
    std::fs::write(&importer, "import { subValue } from 'pkg/feature';\n").expect("write importer");

    let compile_packages = HashSet::from(["pkg".to_string()]);
    let resolved = resolve_import(
        "pkg/feature",
        &importer,
        root,
        &compile_packages,
        &HashMap::new(),
    )
    .expect("resolve pkg/feature");

    assert_eq!(resolved.1, ModuleKind::NativeCompiled);
    assert_eq!(
        resolved.0,
        package_dir
            .join("src/feature/server.ts")
            .canonicalize()
            .expect("canonical subpath")
    );
}

#[test]
fn compile_package_dir_uses_path_components() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let path = root
        .join("node_modules")
        .join("@noble")
        .join("curves")
        .join("node_modules")
        .join("@noble")
        .join("hashes")
        .join("src")
        .join("sha256.ts");

    assert_eq!(
        extract_compile_package_dir(&path, "@noble/hashes").expect("package dir"),
        root.join("node_modules")
            .join("@noble")
            .join("curves")
            .join("node_modules")
            .join("@noble")
            .join("hashes")
    );
    assert_eq!(
        extract_compile_package_dir(&path, "@noble/curves").expect("outer package dir"),
        root.join("node_modules").join("@noble").join("curves")
    );
}
