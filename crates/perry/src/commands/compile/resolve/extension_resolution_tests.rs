//! #6535: extension resolution must map an explicit JS module extension to its
//! OWN TypeScript source form (`.cjs`→`.cts`, `.mjs`→`.mts`, `.js`→`.ts`) — not
//! blanket-redirect every JS extension to a bare `.ts`. The blanket form made
//! `require("./x.cjs")` beside a same-basename `x.ts` entry resolve to that
//! `.ts` (the requiring module itself), so the CJS module was never compiled and
//! its exports read back `undefined`.

use super::{resolve_with_extensions, ts_source_counterparts};

#[test]
fn explicit_cjs_beside_same_basename_ts_resolves_to_cjs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.cjs"), "module.exports = {};\n").unwrap();
    std::fs::write(root.join("mod.ts"), "export const x = 1;\n").unwrap();

    // The `.cjs`'s TS counterpart is `.cts` (absent here), so the actual
    // `.cjs` wins — the bare `.ts` sibling must NOT shadow it.
    let resolved = resolve_with_extensions(&root.join("mod.cjs")).expect("resolve");
    assert_eq!(resolved, root.join("mod.cjs"));
}

#[test]
fn explicit_cjs_prefers_cts_source_when_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.cjs"), "module.exports = {};\n").unwrap();
    std::fs::write(root.join("mod.cts"), "export const x = 1;\n").unwrap();
    // A same-basename `.ts` must not win over the real `.cts` counterpart.
    std::fs::write(root.join("mod.ts"), "export const x = 2;\n").unwrap();

    let resolved = resolve_with_extensions(&root.join("mod.cjs")).expect("resolve");
    assert_eq!(resolved, root.join("mod.cts"));
}

#[test]
fn explicit_mjs_beside_same_basename_ts_resolves_to_mjs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.mjs"), "export default 1;\n").unwrap();
    std::fs::write(root.join("mod.ts"), "export const x = 1;\n").unwrap();

    let resolved = resolve_with_extensions(&root.join("mod.mjs")).expect("resolve");
    assert_eq!(resolved, root.join("mod.mjs"));
}

#[test]
fn explicit_js_still_prefers_ts_source() {
    // The load-bearing `.js`→`.ts` preference is unchanged: an explicit
    // `.js` specifier still compiles a co-located `.ts` source over a
    // (possibly stale) `.js` artifact.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.js"), "export const x = 1;\n").unwrap();
    std::fs::write(root.join("mod.ts"), "export const x = 2;\n").unwrap();

    let resolved = resolve_with_extensions(&root.join("mod.js")).expect("resolve");
    assert_eq!(resolved, root.join("mod.ts"));
}

#[test]
fn bare_specifier_resolves_to_cts_source() {
    // A bare (extensionless) specifier must reach a `.cts` source directly,
    // consistent with `.mts`/`.ts` — `.cts` is in `all_extensions`.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(root.join("mod.cts"), "export const x = 1;\n").unwrap();

    let resolved = resolve_with_extensions(&root.join("mod")).expect("resolve");
    assert_eq!(resolved, root.join("mod.cts"));
}

#[test]
fn ts_source_counterparts_map_each_js_extension_to_its_own_source() {
    assert_eq!(ts_source_counterparts("cjs").to_vec(), vec![".cts"]);
    assert_eq!(ts_source_counterparts("mjs").to_vec(), vec![".mts"]);
    assert_eq!(
        ts_source_counterparts("js").to_vec(),
        vec![".ts", ".tsx", ".mts"]
    );
    assert_eq!(
        ts_source_counterparts("jsx").to_vec(),
        vec![".ts", ".tsx", ".mts"]
    );
    assert!(ts_source_counterparts("ts").is_empty());
    assert!(ts_source_counterparts("json").is_empty());
}
