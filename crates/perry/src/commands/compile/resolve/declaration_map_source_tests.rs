//! Issue #2569 step 5 — prefer the ORIGINAL TypeScript source when a package's
//! build shipped a declaration map (`*.d.ts.map`) or source map (`*.js.map`)
//! recording it. The name-based `src/ ⇄ dist/` heuristics in
//! `resolve_package_source_entry` cannot recover a source whose layout does not
//! mirror the emit; the map can, because it stores the exact original path.

use super::resolve_package_source_entry;
use std::fs;
use std::path::{Path, PathBuf};

/// Write a `compilePackages`-shaped package that ships `dist/index.js` +
/// `dist/index.d.ts`, but NO source file — each test writes the source it
/// needs. Deliberately writes no `src/index.ts`, so the positive map tests
/// below fail unless map-guided resolution actually fires (a `src/index.ts`
/// would let the legacy naming convention pass them regardless).
fn write_dist_package(root: &Path) -> PathBuf {
    let package_dir = root.join("node_modules").join("typed-dist");
    fs::create_dir_all(package_dir.join("dist")).expect("mkdir dist");
    fs::write(package_dir.join("dist/index.js"), "export class Codex {}\n").expect("js");
    fs::write(
        package_dir.join("dist/index.d.ts"),
        "export declare class Codex {}\n",
    )
    .expect("dts");
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
    package_dir
}

/// Write a TypeScript source at `package_dir/rel` (creating parents) and return
/// its canonical path.
fn write_source(package_dir: &Path, rel: &str) -> PathBuf {
    let path = package_dir.join(rel);
    fs::create_dir_all(path.parent().expect("source parent")).expect("mkdir source parent");
    fs::write(&path, "export class Codex {}\n").expect("write source");
    path.canonicalize().expect("canonical source")
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
    let package_dir = write_dist_package(dir.path());
    // Source lives at a non-conventional path the naming heuristics can't find
    // (not `src/`, not a `dist/ → src/` mirror) — only the map records it.
    let original = write_source(&package_dir, "internal/entry.ts");
    fs::write(
        package_dir.join("dist/index.d.ts.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.d.ts",
            "sourceRoot": "",
            "sources": ["../internal/entry.ts"],
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
    let package_dir = write_dist_package(dir.path());
    let original = write_source(&package_dir, "internal/entry.ts");
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../internal/entry.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolved_source(&package_dir), Some(original));
}

#[test]
fn hybrid_cjs_emit_input_keeps_published_javascript_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    let original = package_dir.join("internal/entry.ts");
    fs::create_dir_all(original.parent().expect("source parent")).expect("mkdir source parent");
    fs::write(
        &original,
        "class Ajv {}\nmodule.exports = exports = Ajv\nexport default Ajv\n",
    )
    .expect("write hybrid source");
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../internal/entry.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolve_package_source_entry(&package_dir, None), None);
}

#[test]
fn hybrid_src_index_convention_keeps_published_javascript_entry() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    fs::create_dir_all(package_dir.join("src")).expect("mkdir src");
    fs::write(
        package_dir.join("src/index.ts"),
        "const plugin = () => {}\nmodule.exports = exports = plugin\nexport default plugin\n",
    )
    .expect("write hybrid source");

    assert_eq!(resolve_package_source_entry(&package_dir, None), None);
}

#[test]
fn hybrid_package_root_keeps_subpaths_on_published_javascript() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    let root_source = write_source(&package_dir, "internal/root.ts");
    fs::write(
        &root_source,
        "class Ajv {}\nmodule.exports = exports = Ajv\nexport default Ajv\n",
    )
    .expect("write hybrid root");
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../internal/root.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write root map");

    fs::write(package_dir.join("dist/helper.js"), "exports.helper = 1;\n").expect("helper js");
    write_source(&package_dir, "internal/helper.ts");
    fs::write(
        package_dir.join("dist/helper.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "helper.js",
            "sources": ["../internal/helper.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write helper map");

    assert_eq!(
        resolve_package_source_entry(&package_dir, Some("dist/helper")),
        None
    );
}

#[test]
fn nested_cjs_factory_does_not_defeat_esm_source_redirect() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    let original = package_dir.join("internal/entry.ts");
    fs::create_dir_all(original.parent().expect("source parent")).expect("mkdir source parent");
    fs::write(
        &original,
        "export function helper() {\n  module.exports = factory()\n}\n",
    )
    .expect("write esm source");
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../internal/entry.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(
        resolved_source(&package_dir),
        Some(original.canonicalize().expect("canonical source"))
    );
}

#[test]
fn source_root_is_prepended_to_map_sources() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    let original = write_source(&package_dir, "internal/entry.ts");
    // sourceRoot "../internal" + source "entry.ts", both relative to dist/.
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sourceRoot": "../internal",
            "sources": ["entry.ts"],
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
    let package_dir = write_dist_package(dir.path());
    // The convention target that must win when the map is unusable.
    let convention = write_source(&package_dir, "src/index.ts");
    // A dist-only tarball: the map still records `../original/missing.ts`, but
    // that file was never published. Resolution must not break — it falls
    // through to the `src/index.ts` naming convention.
    fs::write(
        package_dir.join("dist/index.js.map"),
        serde_json::json!({
            "version": 3,
            "file": "index.js",
            "sources": ["../original/missing.ts"],
            "names": [],
            "mappings": ""
        })
        .to_string(),
    )
    .expect("write js.map");

    assert_eq!(resolved_source(&package_dir), Some(convention));
}

#[test]
fn no_map_uses_existing_src_index_convention() {
    // With no map at all, behavior is unchanged: the `src/index.ts` convention
    // still wins. Guards against a regression in the reorder.
    let dir = tempfile::tempdir().expect("tempdir");
    let package_dir = write_dist_package(dir.path());
    let convention = write_source(&package_dir, "src/index.ts");
    assert_eq!(resolved_source(&package_dir), Some(convention));
}
