//! Regression: a closure created EARLIER in a NESTED block (`try` / `{}` /
//! loop body / switch case) that forward-references a `let`/`const` declared
//! LATER in that SAME block was globalized instead of captured, so the
//! reference threw `ReferenceError: <name> is not defined` at runtime.
//!
//! Root cause: `pre_register_forward_captured_lets` (perry-hir
//! `lower_decl/block.rs`) only pre-registered forward-captured lexical bindings
//! at the FUNCTION-BODY top level. Nested block scopes were never scanned, so
//! an earlier closure literal in a nested block captured a `globalThis` read of
//! the not-yet-declared name. Fix: process a worklist of block statement-lists
//! (top level plus every nested block scope) so the forward-captured box is
//! preallocated at function entry and the closure captures it.
//!
//! Minimal shape (both a bare `try` and inside an async generator, the compiled
//! streaming-idle-timeout closures that first surfaced this):
//!
//!   function f() { try { let cb = () => q; let q = 5; return cb(); } finally {} }

use std::process::Command;

fn perry_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let out = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&out).output().expect("run compiled binary");
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    let stderr = String::from_utf8_lossy(&run.stderr).to_string();
    assert!(
        run.status.success(),
        "compiled binary exited non-zero: {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        run.status
    );
    // The bug manifested as this throw reaching stderr while the process exited 0
    // (uncaught-in-promise) or non-zero; guard both.
    assert!(
        !stderr.contains("is not defined"),
        "forward-captured binding in a nested block globalized (ReferenceError):\n{stderr}"
    );
    stdout
}

/// Closure in a `try` block forward-referencing a `let` declared later in the
/// same block. Controls: `g` (no nesting — always worked) and `h` (backward
/// reference — always worked).
#[test]
fn closure_in_try_forward_references_later_let() {
    let stdout = compile_and_run(
        r#"
function f(): number {
  try {
    let cb = () => q6;
    let q6 = 5;
    return cb();
  } finally {}
}
function g(): number { let cb = () => q6; let q6 = 6; return cb(); }
function h(): number { try { let q6 = 7; let cb = () => q6; return cb(); } finally {} }
console.log(`${f()} ${g()} ${h()}`);
"#,
    );
    assert_eq!(stdout.trim(), "5 6 7");
}

/// The shape that first surfaced this: an async generator whose nested-block
/// closures forward-reference (and mutate) later-declared locals.
#[test]
fn async_generator_nested_block_forward_capture() {
    let stdout = compile_and_run(
        r#"
async function* gen(a: number): AsyncGenerator<number> {
  await Promise.resolve();
  yield a;
  try {
    let start = () => { flag = true; };
    let read = () => (flag ? val : 0);
    let flag = false, val = 41;
    start();
    yield a + read();
  } finally {}
}
(async () => {
  let s = 0;
  for await (const x of gen(1)) s += x;
  console.log(s);
})();
"#,
    );
    assert_eq!(stdout.trim(), "43");
}
