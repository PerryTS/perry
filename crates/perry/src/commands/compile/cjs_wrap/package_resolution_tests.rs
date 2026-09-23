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
        !wrapped_unapproved.contains("node_modules/dual-entry/index.js"),
        "require-condition resolution must not pull an unapproved package into native compilation:\n{wrapped_unapproved}"
    );
}
