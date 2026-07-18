//! Issue #2569 step 5 — prefer the ORIGINAL TypeScript source when a package's
//! build shipped a declaration map (`*.d.ts.map`) or source map (`*.js.map`)
//! recording it. The name-based `src/ ⇄ dist/` heuristics in
//! `resolve_package_source_entry` cannot recover a source whose layout does not
//! mirror the emit; the map can, because it stores the exact original path.

use super::resolve_package_source_entry;
use std::fs;
use std::path::{Path, PathBuf};

/// Write a `compilePackages`-shaped package that ships `dist/index.js` +
/// `dist/index.d.ts` alongside the original `src/index.ts`. Map files are added
/// per-test. Returns the package dir and the canonical original source path.
fn write_dist_package(root: &Path) -> (PathBuf, PathBuf) {
    let package_dir = root.join("node_modules").join("typed-dist");
    fs::create_dir_all(package_dir.join("dist")).expect("mkdir dist");
    fs::create_dir_all(package_dir.join("src")).expect("mkdir src");
    fs::write(package_dir.join("dist/index.js"), "export class Codex {}\n").expect("js");
    fs::write(
        package_dir.join("dist/index.d.ts"),
        "export declare class Codex {}\n",
    )
    .expect("dts");
    let original = package_dir.join("src/index.ts");
    fs::write(&original, "export class Codex {}\n").expect("ts");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({
            "name": "typed-dist",
            "type": "module",
            "module": "./dist/index.js",
            "types": "./dist/index.d.ts",
            "exports": { ".": { "types": "./dist/index.d.ts", "import": "./dist/index.js" } }
        })
        .to_string(),
    )
    .expect("package.json");
    (package_dir, original.canonicalize().expect("canonical src"))
}

/// Canonicalize the result so the assertion is robust both to the naming-
/// convention path returning a non-canonical path and to macOS resolving
/// `/var` → `/private/var`.
fn resolved_source(package_dir: &Path) -> Option<PathBuf> {
    resolve_package_source_entry(package_dir, None)
        .map(|p| p.canonicalize().expect("canonical resolved source"))
}

#[test]
fn declaration_map_redirects_to_original_typescript_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, original) = write_dist_package(dir.path());
    fs::write(
        package_dir.join("dist/index.d.ts.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.d.ts",
            "sourceRoot": "",
            "sources": ["../src/index.ts"],
            "names": []
        })
        .to_string(),
    )
    .expect("write d.ts.map");

    assert_eq!(resolved_source(&package_dir), Some(original));
}

#[test]
fn source_map_redirects_when_no_declaration_map() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, original) = write_dist_package(dir.path());
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../src/index.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolved_source(&package_dir), Some(original));
}

#[test]
fn source_root_is_prepended_to_map_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, original) = write_dist_package(dir.path());
    // sourceRoot "../src" + source "index.ts", both relative to dist/.
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sourceRoot": "../src",
            "sources": ["index.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolved_source(&package_dir), Some(original));
}

#[test]
fn bundled_multi_source_map_is_not_redirected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = dir.path().join("node_modules").join("bundled");
    fs::create_dir_all(package_dir.join("dist")).expect("mkdir dist");
    fs::create_dir_all(package_dir.join("lib-src")).expect("mkdir lib-src");
    fs::write(package_dir.join("dist/index.js"), "export const x = 1;\n").expect("js");
    fs::write(package_dir.join("lib-src/a.ts"), "export const a = 1;\n").expect("a");
    fs::write(package_dir.join("lib-src/b.ts"), "export const b = 2;\n").expect("b");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "bundled", "type": "module", "module": "./dist/index.js" })
            .to_string(),
    )
    .expect("package.json");
    // A bundle folds many inputs into one output: several `sources`. There is
    // no single original source to compile in place of the emit, and no
    // `src/index.ts` for the name conventions to fall back to — so the result
    // must be None rather than an arbitrary pick of one input.
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../lib-src/a.ts", "../lib-src/b.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolve_package_source_entry(&package_dir, None), None);
}

#[test]
fn map_pointing_at_missing_source_falls_back_to_convention() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, original) = write_dist_package(dir.path());
    // A dist-only tarball: the map still records `../original/index.ts`, but
    // that file was never published. Resolution must not break — it falls
    // through to the `src/index.ts` naming convention, which is present.
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../original/index.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolved_source(&package_dir), Some(original));
}

#[test]
fn no_map_uses_existing_src_index_convention() {
    // With no map at all, behavior is unchanged: the `src/index.ts` convention
    // still wins. Guards against a regression in the reorder.
    let dir = tempfile::tempdir().expect("tempdir");
    let (package_dir, original) = write_dist_package(dir.path());
    assert_eq!(resolved_source(&package_dir), Some(original));
}
