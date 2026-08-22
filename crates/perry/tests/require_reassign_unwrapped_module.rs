//! Regression test: a genuinely top-level (unwrapped) CJS module that does
//! `let x = require("<native module>"); x = someHelper.__toESM(x);` must keep
//! a real runtime binding for `x`.
//!
//! This is the exact shape of `@socketsecurity/lib`'s rolldown-bundled
//! `dist/bin/trusted.js`: a plain CJS file (no enclosing `cjs_wrap` IIFE, no
//! synthetic `require`) whose top level reassigns the `require()` result to
//! wrap it in a `.default` shape, then reads `x.default.*` further down.
//!
//! Pre-fix, `register_native_fetch_and_streams` treated ANY `let/const/var
//! x = require("<native>")` as an immutable namespace binding and emitted no
//! runtime variable for `x` at all — correct for a `const` that's never
//! reassigned, wrong here: the very next statement's `x = require_runtime
//! .__toESM(x)` then referenced a binding that was never created, and the
//! compiled binary threw `ReferenceError: node_process is not defined` on
//! every invocation even though the compile itself reported success. The fix
//! gates the optimization on the declaration actually being `const`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, entry_name: &str) -> String {
    let entry = dir.join(entry_name);
    let output = dir.join("main_bin");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
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

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        run.stderr.is_empty(),
        "compiled binary wrote to stderr\nstderr:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn unwrapped_cjs_package_reassigning_native_require_keeps_runtime_binding() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "require-reassign-unwrapped",
  "type": "module",
  "perry": {
    "compilePackages": ["trusted-pkg"],
    "allow": { "compilePackages": ["trusted-pkg"] }
  }
}"#,
    )
    .expect("write root package.json");

    let pkg = root.join("node_modules").join("trusted-pkg");
    std::fs::create_dir_all(&pkg).expect("mkdir trusted-pkg");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "trusted-pkg", "version": "1.0.0", "main": "./trusted.js" }"#,
    )
    .expect("write trusted-pkg package.json");

    // Mirrors socket-lib's `dist/_virtual/_rolldown/runtime.js`: a plain CJS
    // helper, required by relative path.
    std::fs::write(
        pkg.join("runtime.js"),
        r#""use strict";
exports.__toESM = function (mod) {
  if (mod && typeof mod === "object" && mod.__esModule) return mod;
  var target = {};
  Object.defineProperty(target, "default", { value: mod, enumerable: true });
  return target;
};
"#,
    )
    .expect("write runtime.js");

    // Mirrors socket-lib's `dist/bin/trusted.js` exactly: the
    // `Symbol.toStringTag` ESM-interop marker (not the wrap-triggering
    // `__esModule` string key), a relative require for the local helper, then
    // the `let x = require(...); x = helper.__toESM(x);` reassignment this
    // regression is about.
    std::fs::write(
        pkg.join("trusted.js"),
        r#""use strict";
Object.defineProperty(exports, Symbol.toStringTag, { value: "Module" });
const require_runtime = require("./runtime.js");
let node_process = require("node:process");
node_process = require_runtime.__toESM(node_process);

function currentPlatform() {
  return node_process.default.platform;
}
exports.currentPlatform = currentPlatform;
"#,
    )
    .expect("write trusted.js");

    std::fs::write(
        root.join("main.ts"),
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const trusted = require("trusted-pkg");
console.log("platform:", trusted.currentPlatform());
"#,
    )
    .expect("write entry");

    let stdout = compile_and_run(root, "main.ts");
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(stdout, format!("platform: {expected_platform}\n"));
}
