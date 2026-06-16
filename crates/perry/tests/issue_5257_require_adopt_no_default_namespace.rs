//! Regression test for #5257: a `const x = require('pkg')` (or
//! `const { a } = require('pkg')`) that the CJS→ESM wrap adopts as a DEFAULT
//! import (`import _req_N from 'pkg'`) must NOT be rejected by the static-ESM
//! "does not provide an export named 'default'" gate when `pkg` is a
//! named-only / CJS module.
//!
//! Under CommonJS, `require('pkg')` returns the module's *exports object*
//! (its namespace), so `isexe.sync(...)` / `const { sync } = require('isexe')`
//! work against the named exports even though there is no `default`. Before
//! the fix, the package-default-export gate (bootstrap.rs) bailed the whole
//! build with:
//!
//!   Error: The requested package 'isexe' does not provide an export named
//!   'default' (imported as '_req_0' in .../which/lib/index.js).
//!
//! This blocked cross-spawn, which, execa, and joi. The fix tags every import
//! synthesized by the CJS wrap (`is_adopted_require`) and exempts it from the
//! default gate; codegen already routes a no-`default` default import through
//! the namespace machinery (#4872), so member reads / destructuring resolve
//! per-export.
//!
//! The minimized shape mirrors the real `isexe` trigger exactly: a
//! `"type":"module"` package whose `exports` map's `require` condition
//! resolves to a bundled `__esModule` CJS with named-only exports (no
//! default), consumed across a package boundary via `require("foo").bar()`
//! and via destructuring `const { bar } = require("foo")`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn adopted_require_of_named_only_package_binds_namespace_and_runs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "require-adopt-no-default",
  "type": "module",
  "perry": {
    "compilePackages": ["foo", "consumer"],
    "allow": { "compilePackages": ["foo", "consumer"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    // Package `foo`: a `"type":"module"` package whose `exports` map routes
    // `require` to a bundled `__esModule` CJS with named-only exports (no
    // `default`), and `import` to an ESM with named-only exports. This is the
    // exact `isexe` shape that produced #5257.
    let foo = root.join("node_modules").join("foo");
    std::fs::create_dir_all(&foo).expect("mkdir foo");
    std::fs::write(
        foo.join("package.json"),
        r#"{
  "name": "foo",
  "version": "1.0.0",
  "type": "module",
  "exports": {
    ".": {
      "require": "./cjs.js",
      "import": "./esm.js"
    }
  }
}"#,
    )
    .expect("write foo package.json");
    std::fs::write(
        foo.join("cjs.js"),
        r#""use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.bar = void 0;
function bar() { return 42; }
exports.bar = bar;
exports.baz = function baz() { return 7; };
"#,
    )
    .expect("write foo cjs.js");
    std::fs::write(
        foo.join("esm.js"),
        "export const bar = () => 42;\nexport const baz = () => 7;\n",
    )
    .expect("write foo esm.js");

    // Package `consumer`: a CJS module that pulls `foo` in BOTH adopted-require
    // shapes — whole-value alias member access and destructuring.
    let consumer = root.join("node_modules").join("consumer");
    std::fs::create_dir_all(&consumer).expect("mkdir consumer");
    std::fs::write(
        consumer.join("package.json"),
        r#"{ "name": "consumer", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write consumer package.json");
    std::fs::write(
        consumer.join("index.js"),
        r#""use strict";
const foo = require("foo");
const { bar, baz } = require("foo");
module.exports.total = function total() {
    return foo.bar() + bar() + baz();
};
"#,
    )
    .expect("write consumer index.js");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { total } from "consumer";
console.log(total());
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (default-export gate regressed?)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    // The specific #5257 symptom must be gone.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(
        !combined.contains("does not provide an export named 'default'"),
        "the no-default gate must not fire for an adopted require:\n{combined}"
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "91\n",
        "adopted require of a named-only package must bind the exports object \
         (foo.bar()=42 + bar()=42 + baz()=7)"
    );
}

#[test]
fn module_exports_fn_default_require_still_works() {
    // Regression guard: a CJS package whose whole module IS a function
    // (`module.exports = fn`) — i.e. a real `default`-shaped export — must
    // still adopt as a callable default, not get broken by the namespace
    // fallback.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "require-default-fn",
  "type": "module",
  "perry": {
    "compilePackages": ["fnpkg", "user"],
    "allow": { "compilePackages": ["fnpkg", "user"] }
  }
}"#,
    )
    .expect("write package.json");

    let fnpkg = root.join("node_modules").join("fnpkg");
    std::fs::create_dir_all(&fnpkg).expect("mkdir fnpkg");
    std::fs::write(
        fnpkg.join("package.json"),
        r#"{ "name": "fnpkg", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write fnpkg package.json");
    std::fs::write(
        fnpkg.join("index.js"),
        "\"use strict\";\nmodule.exports = function add(a, b) { return a + b; };\n",
    )
    .expect("write fnpkg index.js");

    let user = root.join("node_modules").join("user");
    std::fs::create_dir_all(&user).expect("mkdir user");
    std::fs::write(
        user.join("package.json"),
        r#"{ "name": "user", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write user package.json");
    std::fs::write(
        user.join("index.js"),
        "\"use strict\";\nconst add = require(\"fnpkg\");\nmodule.exports.run = function run() { return add(40, 2); };\n",
    )
    .expect("write user index.js");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        "import { run } from \"user\";\nconsole.log(run());\n",
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "42\n",
        "module.exports = fn must still adopt as a callable default"
    );
}
