//! #8342: a CJS-wrapped module that does a bare top-level
//! `require("process")` (and other Node.js built-ins) must resolve the
//! built-in to the real namespace at runtime via the wrap's synthetic
//! `createRequire`-backed require arm.
//!
//! Pre-fix the HIR's destructuring `var`/`let`/`const` pass intercepted
//! `let node_process = require("process")` BEFORE call lowering and stole it
//! into a native-module namespace binding (`register_require_namespace_binding`
//! → `remove_local_binding`), mirroring `import * as node_process from
//! "process"`. But the codegen does not initialize native-module import
//! bindings inside CJS-wrapped modules, so `node_process` resolved to nothing
//! at runtime — `ReferenceError: node_process is not defined` — which blocked
//! `sdxgen --help` (the rolldown-bundled `@socketsecurity/lib` external-pack.js
//! starts with `let node_process = require("process"); node_process =
//! __toESM(node_process, 1)`).
//!
//! The fix gates the destructuring native-require fast path on `require` being
//! the bare global (not shadowed by the wrap's synthetic `function require`),
//! and the wrap no longer hoists a static `import _req_N from '<builtin>'` for
//! built-in specs. The body's `require("<builtin>")` call flows through to the
//! synthetic require, whose per-spec case resolves via `createRequire`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile a CJS entry (`.cjs` so the wrap is applied) and run it, returning
/// stdout. Asserts both the compile and the run succeed.
fn compile_and_run_cjs(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.cjs");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

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
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// The simplest witness: a bare `const p = require("process")` in a CJS-wrapped
/// module. Pre-fix this threw `ReferenceError: p is not defined` when the body
/// read `p.platform`.
#[test]
fn cjs_wrap_bare_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const p = require("process");
const os = require("os");
const path = require("path");
console.log("platform:", p.platform);
console.log("cpus:", typeof os.cpus);
console.log("join:", typeof path.join, path.join("a", "b"));
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!("platform: {expected_platform}\ncpus: function\njoin: function a/b\n")
    );
}

/// The exact sdxgen/rolldown shape: `let node_process = require("process");
/// node_process = __toESM(node_process, 1)` — the alias is REASSIGNED, so the
/// wrap can't adopt it, and the HIR's destructuring pass stole it into a
/// native-module namespace binding (dropping the runtime local). Pre-fix:
/// `ReferenceError: node_process is not defined` on every invocation.
#[test]
fn cjs_wrap_rolldown_toesm_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
var __toESM = (mod, isNodeMode) => {
  if (mod && typeof mod === "object" && mod.__esModule) return mod;
  var target = {};
  Object.defineProperty(target, "default", { value: mod, enumerable: true });
  return Object.assign(target, mod);
};
let node_process = require("process");
node_process = __toESM(node_process, 1);
let node_os = require("os");
node_os = __toESM(node_os, 1);
console.log("node_process.platform:", node_process.platform);
console.log("node_os.cpus:", typeof node_os.cpus);
module.exports = { platform: node_process.platform };
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!("node_process.platform: {expected_platform}\nnode_os.cpus: function\n")
    );
}

/// Destructured built-in require: `const { platform } = require("process")`
/// in a CJS-wrapped module. The destructuring native-require fast path must
/// also bail when `require` is shadowed by the wrap's synthetic require, so
/// `platform` binds from the runtime `require("process")` result instead of a
/// native-module alias that isn't initialized in a CJS-wrapped module.
#[test]
fn cjs_wrap_destructured_builtin_require_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run_cjs(
        dir.path(),
        r#"
const { platform, arch } = require("process");
const { join } = require("path");
console.log("platform:", platform);
console.log("arch:", typeof arch);
console.log("join:", join("a", "b"));
"#,
    );
    let platform = std::env::consts::OS;
    let expected_platform = match platform {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        _ => platform,
    };
    assert_eq!(
        stdout,
        format!("platform: {expected_platform}\narch: string\njoin: a/b\n")
    );
}
