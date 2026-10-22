//! A compile package subpath named by its package.json `exports` map resolves
//! to the exports target, as in Node and Bun, even when a sibling file is named
//! like the subpath (`./mini/tool` -> `src/mini/tool.public.ts` next to
//! `src/mini/tool.ts`, the `@opencode/tui` shape).

use super::resolve_package_source_entry;
use std::fs;
use std::path::{Path, PathBuf};

fn write_package(root: &Path, exports: serde_json::Value, files: &[&str]) -> PathBuf {
    let package_dir = root.join("node_modules").join("subpath-pkg");
    for rel in files {
        let path = package_dir.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(&path, "export const x = 1;\n").expect("write file");
    }
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "subpath-pkg", "type": "module", "exports": exports })
            .to_string(),
    )
    .expect("package.json");
    package_dir
}

fn resolved(package_dir: &Path, subpath: &str) -> PathBuf {
    resolve_package_source_entry(package_dir, Some(subpath))
        .expect("subpath resolves")
        .canonicalize()
        .expect("canonical")
}

fn canonical(package_dir: &Path, rel: &str) -> PathBuf {
    package_dir
        .join(rel)
        .canonicalize()
        .expect("canonical file")
}

#[test]
fn exports_target_wins_over_same_named_src_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(
        dir.path(),
        serde_json::json!({ ".": "./src/index.ts", "./mini/tool": "./src/mini/tool.public.ts" }),
        &[
            "src/index.ts",
            "src/mini/tool.ts",
            "src/mini/tool.public.ts",
        ],
    );
    assert_eq!(
        resolved(&pkg, "mini/tool"),
        canonical(&pkg, "src/mini/tool.public.ts")
    );
}

#[test]
fn exports_js_target_still_maps_to_its_source_mirror() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(
        dir.path(),
        serde_json::json!({ "./helper": { "import": "./dist/helper.js" } }),
        &["dist/helper.js", "src/helper.ts"],
    );
    assert_eq!(resolved(&pkg, "helper"), canonical(&pkg, "src/helper.ts"));
}

#[test]
fn src_shorthand_applies_when_exports_does_not_name_the_subpath() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pkg = write_package(
        dir.path(),
        serde_json::json!("./src/index.ts"),
        &["src/index.ts", "src/extra.ts"],
    );
    assert_eq!(resolved(&pkg, "extra"), canonical(&pkg, "src/extra.ts"));
}
