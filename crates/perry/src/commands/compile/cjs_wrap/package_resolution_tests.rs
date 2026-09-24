use super::wrap::wrap_commonjs_for_target;
use std::collections::HashSet;
use std::fs;

/// Issue #11047: once a CommonJS `require("pkg")` is wrapped as an ESM import,
/// package resolution must retain require-call semantics. `ws` exposes an ESM
/// default under `exports.import`, but only its `exports.require` entry adds
/// the legacy `WebSocket.Server` property.
#[test]
fn cjs_bare_require_uses_package_require_export_condition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let package = dir.path().join("node_modules/dual-entry");
    fs::create_dir_all(&package).expect("create package");
    fs::write(
        package.join("package.json"),
        r#"{
  "name": "dual-entry",
  "exports": {
    ".": {
      "import": "./wrapper.mjs",
      "require": "./index.js"
    }
  }
}"#,
    )
    .expect("write package.json");
    fs::write(
        package.join("wrapper.mjs"),
        "export default class WebSocket {}\n",
    )
    .expect("write ESM entry");
    fs::write(
        package.join("index.js"),
        "module.exports = class WebSocket {};\n",
    )
    .expect("write CJS entry");

    let entry = dir.path().join("entry.js");
    let compile_packages = HashSet::from(["dual-entry".to_string()]);
    let wrapped = wrap_commonjs_for_target(
        "const WebSocket = require('dual-entry');\nmodule.exports = WebSocket;\n",
        &entry,
        None,
        false,
        Some(&compile_packages),
    );
    let expected = "node_modules/dual-entry/index.js\";";
    assert!(
        wrapped.contains(expected),
        "expected require-condition entry ending in `{expected}`, got:\n{wrapped}"
    );
    assert!(
        !wrapped.contains("wrapper.mjs"),
        "CommonJS require must not select the import-condition entry:\n{wrapped}"
    );

    let wrapped_unapproved = wrap_commonjs_for_target(
        "module.exports = require('dual-entry');\n",
        &entry,
        None,
        false,
        Some(&HashSet::new()),
    );
    assert!(
        wrapped_unapproved.contains("from 'dual-entry';"),
        "a package outside compilePackages must retain its bare specifier:\n{wrapped_unapproved}"
    );
    assert!(
        !wrapped_unapproved.lines().filter(|line| line.starts_with("import ")).any(|line| line.contains("node_modules/dual-entry/index.js")),
        "require-condition resolution must not pull an unapproved package into native compilation:\n{wrapped_unapproved}"
    );
}

#[test]
fn require_resolve_static_files_return_canonical_filenames() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("folder")).unwrap();
    fs::write(dir.path().join("m.js"), "module.exports = 42;").unwrap();
    fs::write(dir.path().join("folder/index.js"), "module.exports = 7;").unwrap();
    for (specifier, filename) in [
        ("./m.js", "m.js"),
        ("./m", "m.js"),
        ("./folder", "folder/index.js"),
    ] {
        let source = format!("const value = require({specifier:?}); module.exports = require.resolve({specifier:?});");
        let wrapped =
            wrap_commonjs_for_target(&source, &dir.path().join("main.cjs"), None, false, None);
        let expected = dir.path().join(filename).canonicalize().unwrap();
        let case = format!(
            "if (specifier === {}) return {};",
            serde_json::to_string(specifier).unwrap(),
            serde_json::to_string(&expected.to_string_lossy()).unwrap()
        );
        assert!(
            wrapped.contains(&case),
            "expected resolved filename case {case}:\n{wrapped}"
        );
    }
}

#[test]
fn require_resolve_missing_optional_file_has_no_success_case() {
    let dir = tempfile::tempdir().unwrap();
    let wrapped = wrap_commonjs_for_target(
        "try { require('./missing.js'); } catch (_) {}",
        &dir.path().join("main.cjs"),
        None,
        false,
        None,
    );
    assert!(
        !wrapped.contains("if (specifier === \"./missing.js\") return \"./missing.js\";"),
        "an unresolved optional file must reach MODULE_NOT_FOUND:\n{wrapped}"
    );
}

#[test]
fn require_resolve_builtin_identity_beats_installed_package() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("node_modules/fs");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("index.js"), "module.exports = {}; ").unwrap();
    let wrapped = wrap_commonjs_for_target(
        "require('fs'); require('node:fs');",
        &dir.path().join("main.cjs"),
        None,
        false,
        None,
    );
    for specifier in ["fs", "node:fs"] {
        let encoded = serde_json::to_string(specifier).unwrap();
        assert!(
            wrapped.contains(&format!("if (specifier === {encoded}) return {encoded};")),
            "builtin must keep its identity:\n{wrapped}"
        );
    }
}
