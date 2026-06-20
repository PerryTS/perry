//! Regression test for #5257 — default-imported CJS Node builtins dispatch
//! their member methods.
//!
//! `const cp = require('child_process')` is adopted as `import cp from
//! 'child_process'`. A CJS builtin's default import binds the whole module
//! namespace, so `cp.spawnSync(...)` must resolve through the builtin-module
//! alias — exactly as `import * as cp from 'child_process'` already did. The
//! default-import path only registered that alias for `process`, so
//! `cp.spawnSync(...)` lowered to nothing and returned `undefined`, breaking
//! cross-spawn / execa / which (via isexe) under the require() adoption rewrite.
//!
//! Fixed in `lower/module_decl.rs`: register the builtin-module alias for
//! every CJS-style native default import, not just `process`.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_ts(src: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    std::fs::write(&entry, src).expect("write");
    let out = dir.path().join("bin");
    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("compile");
    assert!(
        compile.status.success(),
        "compile failed:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let run = Command::new(&out).output().expect("run");
    assert!(
        run.status.success(),
        "compiled binary exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).to_string()
}

// POSIX-only: spawns `echo` directly (no shell), which isn't a standalone
// executable on Windows, and asserts a status-0/stdout shape specific to it.
// The fix under test (default-import alias registration) is platform-agnostic;
// this just exercises it through a concrete builtin call.
#[cfg(unix)]
#[test]
fn default_imported_child_process_spawn_sync_dispatches() {
    // `cp.spawnSync` via a DEFAULT import (the require()-adoption shape) must
    // return the result object, not `undefined`.
    let stdout = run_ts(
        r#"
import cp from "node:child_process"
const r: any = cp.spawnSync("echo", ["perry5257"], { encoding: "utf8" })
console.log("typeof:", typeof r)
console.log("status:", r && r.status)
console.log("stdout:", r && (r.stdout || "").toString().trim())
"#,
    );
    assert!(stdout.contains("typeof: object"), "got:\n{stdout}");
    assert!(stdout.contains("status: 0"), "got:\n{stdout}");
    assert!(stdout.contains("stdout: perry5257"), "got:\n{stdout}");
}

#[test]
fn default_imported_cjs_builtins_still_dispatch_methods() {
    // Regression guard: other CJS-style builtins keep working through the
    // default-import alias.
    let stdout = run_ts(
        r#"
import path from "node:path"
import util from "node:util"
// Normalize the separator so the assertion is platform-agnostic (Windows
// joins with "\", POSIX with "/").
console.log("join:", path.join("a", "b", "c").split(path.sep).join("/"))
console.log("fmt:", util.format("%s-%d", "x", 7))
"#,
    );
    assert!(stdout.contains("join: a/b/c"), "got:\n{stdout}");
    assert!(stdout.contains("fmt: x-7"), "got:\n{stdout}");
}
