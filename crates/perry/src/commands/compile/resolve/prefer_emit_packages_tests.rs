//! `PERRY_PREFER_EMIT_PACKAGES` keeps named packages (or all of them with `*`)
//! on their emitted entry instead of their TypeScript source.

use super::compile_package_source_entry;
use std::fs;
use std::path::{Path, PathBuf};

fn write_package_with_source_and_emit(root: &Path) -> (PathBuf, PathBuf) {
    let package_dir = root.join("node_modules").join("dual-entry");
    fs::create_dir_all(package_dir.join("src")).expect("mkdir src");
    fs::create_dir_all(package_dir.join("dist")).expect("mkdir dist");
    fs::write(package_dir.join("src/index.ts"), "export const v = 1;\n").expect("ts");
    fs::write(package_dir.join("dist/index.js"), "export const v = 1;\n").expect("js");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({
            "name": "dual-entry",
            "type": "module",
            "main": "./dist/index.js"
        })
        .to_string(),
    )
    .expect("package.json");
    let source = package_dir
        .join("src/index.ts")
        .canonicalize()
        .expect("canonical source");
    (package_dir, source)
}

fn source_entry(package_dir: &Path, setting: Option<&str>) -> Option<PathBuf> {
    compile_package_source_entry(package_dir, "dual-entry", None, setting)
        .map(|p| p.canonicalize().expect("canonical entry"))
}

#[test]
fn prefer_emit_packages_opts_named_packages_out_of_source_preference() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, source) = write_package_with_source_and_emit(dir.path());

    assert_eq!(source_entry(&package_dir, None), Some(source.clone()));
    assert_eq!(source_entry(&package_dir, Some("")), Some(source.clone()));
    assert_eq!(
        source_entry(&package_dir, Some("other-pkg")),
        Some(source.clone()),
        "an unnamed package keeps TypeScript source preference"
    );
    assert_eq!(source_entry(&package_dir, Some("dual-entry")), None);
    assert_eq!(
        source_entry(&package_dir, Some("other-pkg, dual-entry")),
        None
    );
    assert_eq!(source_entry(&package_dir, Some("*")), None);
}
