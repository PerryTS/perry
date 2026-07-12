//! Negative-compile tests for #6185 (Tier-1 cross-heap containment in
//! `perry/thread` worker closures).
//!
//! The codegen walk in `lower_call/closure_analysis.rs` must reject, at
//! compile time, worker closures that:
//!   1. are async (or contain an async closure / `await`) — the emitted
//!      await loop would drain the process-global completion/timer queues
//!      on the worker thread and resolve foreign-heap promises;
//!   2. call a nested `spawn` / `parallelMap` / `parallelFilter`;
//!   3. read a module-scope binding whose value is a heap object — module
//!      globals are process-wide slots read in place (they bypass the
//!      capture deep-copy), so the worker would alias main-heap objects.
//!
//! The companion positive tests pin the sanctioned patterns: primitive
//! module globals, deep-copied local captures, and module-level `function`
//! helpers must keep compiling (and running) exactly as documented in
//! docs/src/threading/overview.md.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source`; expect the compile itself to FAIL with a diagnostic
/// containing `expect_fragment`.
fn compile_expect_error(source: &str, expect_fragment: &str) {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        !compile.status.success(),
        "perry compile unexpectedly succeeded (wanted error containing {:?})\nstdout:\n{}\nstderr:\n{}",
        expect_fragment,
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(
        combined.contains(expect_fragment),
        "compile failed but diagnostic missing {:?}\noutput:\n{}",
        expect_fragment,
        combined
    );
}

/// Compile and run `source`; expect success and return stdout.
fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
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
        .current_dir(dir.path())
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

// ── Negative: async / await inside the worker ────────────────────────────────

/// An `async` worker closure (no awaits) keeps `is_async: true` through the
/// transforms; the check must reject it on the flag alone.
#[test]
fn rejects_async_worker_closure() {
    compile_expect_error(
        r#"
import { spawn } from "perry/thread";
const p = spawn(async () => {
  return 42;
});
console.log(await p);
"#,
        "must be synchronous",
    );
}

/// An await-containing worker closure is CPS-rewritten before codegen
/// (`is_async` cleared, FuncId recorded in `Module::async_step_closures`);
/// the check must catch it through the async-step registry.
#[test]
fn rejects_awaiting_worker_closure() {
    compile_expect_error(
        r#"
import { spawn } from "perry/thread";
async function delay(): Promise<number> { return 7; }
const p = spawn(async () => {
  const v = await delay();
  return v;
});
console.log(await p);
"#,
        "must be synchronous",
    );
}

/// An async closure NESTED inside a sync worker body executes on the worker
/// thread when invoked — same hazard, must also be rejected.
#[test]
fn rejects_nested_async_closure_in_worker() {
    compile_expect_error(
        r#"
import { spawn } from "perry/thread";
const p = spawn(() => {
  const inner = async () => 5;
  inner();
  return 1;
});
console.log(await p);
"#,
        "must be synchronous",
    );
}

// ── Negative: nested thread primitives ───────────────────────────────────────

#[test]
fn rejects_nested_spawn_inside_parallel_map() {
    compile_expect_error(
        r#"
import { spawn, parallelMap } from "perry/thread";
const data = [1, 2, 3];
function run(): number[] {
  return parallelMap(data, (x: number) => {
    spawn(() => x * 2);
    return x;
  });
}
console.log(run().length);
"#,
        "may not be called inside a closure",
    );
}

// ── Negative: heap-typed module-global reads ─────────────────────────────────

/// A module-scope object literal read from inside the worker bypasses the
/// capture deep-copy (module globals are process-wide slots) — reject.
#[test]
fn rejects_module_object_read_in_worker() {
    compile_expect_error(
        r#"
import { parallelMap } from "perry/thread";
const config = { threshold: 5 };
const data = [1, 2, 3, 10];
const out = parallelMap(data, (x: number) => x * config.threshold);
console.log(out);
"#,
        "module-scope variable",
    );
}

/// A module-scope arrow-function helper is a heap closure object on the
/// spawning thread's arena — reject, pointing at the `function` workaround.
#[test]
fn rejects_module_arrow_helper_in_worker() {
    compile_expect_error(
        r#"
import { parallelMap } from "perry/thread";
const double = (x: number): number => x * 2;
const data = [1, 2, 3];
const out = parallelMap(data, (x: number) => double(x));
console.log(out);
"#,
        "module-scope variable",
    );
}

/// Same module-global-read rejection, but with the spawn callsite inside a
/// STATIC method — `compile_static_method` seeds its FnCtx `local_types`
/// from `module_global_types` (it didn't pre-#6185), so the classifier can
/// see the binding's heap type there too.
#[test]
fn rejects_module_object_read_in_static_method_worker() {
    compile_expect_error(
        r#"
import { parallelMap } from "perry/thread";
const config = { threshold: 5 };
class Jobs {
  static run(data: number[]): number[] {
    return parallelMap(data, (x: number) => x * config.threshold);
  }
}
console.log(Jobs.run([1, 2, 3]));
"#,
        "module-scope variable",
    );
}

// ── Positive: sanctioned patterns keep compiling and running ────────────────

/// Primitive module globals stay allowed from a static-method worker
/// closure too (the static-method FnCtx now has the type info to tell the
/// difference, rather than allowing everything by ignorance).
#[test]
fn allows_primitive_module_global_in_static_method_worker() {
    let out = compile_and_run(
        r#"
import { parallelMap } from "perry/thread";
const factor = 4;
class Jobs {
  static run(data: number[]): number[] {
    return parallelMap(data, (x: number) => x * factor);
  }
}
console.log(Jobs.run([1, 2, 3]));
"#,
    );
    assert_eq!(out.trim(), "[ 4, 8, 12 ]");
}

/// Primitive module globals (number, string) are plain 64-bit copies /
/// immutable rooted data — reading them in a worker stays allowed.
#[test]
fn allows_primitive_module_globals_in_worker() {
    let out = compile_and_run(
        r#"
import { parallelMap } from "perry/thread";
const factor = 3;
const label = "x";
const data = [1, 2, 3];
const out = parallelMap(data, (x: number) => x * factor);
console.log(label, out[0], out[1], out[2]);
"#,
    );
    assert_eq!(out.trim(), "x 3 6 9");
}

/// The documented workaround: bind the module global to a function-scope
/// local so the closure captures it and the value is deep-copied.
#[test]
fn allows_local_copy_of_module_object() {
    let out = compile_and_run(
        r#"
import { parallelMap } from "perry/thread";
const config = { threshold: 5 };
const data = [1, 2, 3];
function run(): number[] {
  const threshold = config.threshold;
  return parallelMap(data, (x: number) => x * threshold);
}
console.log(run());
"#,
    );
    assert_eq!(out.trim(), "[ 5, 10, 15 ]");
}

/// Module-level `function` declarations are static code, not heap closure
/// objects — calling them from a worker stays allowed (docs' spawn-multiple
/// pattern).
#[test]
fn allows_module_function_helper_in_worker() {
    let out = compile_and_run(
        r#"
import { parallelMap } from "perry/thread";
function double(x: number): number { return x * 2; }
const data = [1, 2, 3];
const out = parallelMap(data, (x: number) => double(x));
console.log(out);
"#,
    );
    assert_eq!(out.trim(), "[ 2, 4, 6 ]");
}

/// `await spawn(...)` in the ENCLOSING scope is the documented usage — the
/// await is outside the worker body and must not trip the check.
#[test]
fn allows_await_of_spawn_result_outside_worker() {
    let out = compile_and_run(
        r#"
import { spawn } from "perry/thread";
async function main(): Promise<void> {
  const result = await spawn(() => {
    let sum = 0;
    for (let i = 1; i <= 100; i++) sum += i;
    return sum;
  });
  console.log("spawn_result:" + result);
}
main();
"#,
    );
    assert_eq!(out.trim(), "spawn_result:5050");
}
