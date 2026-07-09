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

/// `else if` bodies are `Stmt::If` in the alt position, not `Stmt::Block`, so
/// the original worklist (which only enqueued DIRECT block bodies) never
/// scanned them — the closure globalized and threw. Also covers a block
/// behind a labeled `while` + non-block `if` chain.
#[test]
fn closure_in_else_if_and_labeled_while_forward_references_later_let() {
    let stdout = compile_and_run(
        r#"
function f(n: number): number {
  if (n === 0) {
    return -1;
  } else if (n === 1) {
    let cb = () => q;
    let q = 5;
    return cb();
  }
  return -2;
}
function g(x: number): number {
  outer: while (x > 0) {
    if (x === 1) {
      let cb = () => q;
      let q = 42;
      return cb();
    }
    x--;
  }
  return -1;
}
console.log(`${f(1)} ${g(3)}`);
"#,
    );
    assert_eq!(stdout.trim(), "5 42");
}

/// Same-named forward-captured `let`s in SIBLING blocks must each get their
/// own binding/box (the name-keyed dedup gave both closures the FIRST block's
/// box: `1,1`), and a nested pre-registration must not stay name-visible
/// outside its block — reads before/after the block resolve the OUTER
/// (module) binding, not the block's box or its TDZ sentinel.
#[test]
fn sibling_same_name_blocks_and_no_scope_leak() {
    let stdout = compile_and_run(
        r#"
let q = "module";
function siblings(): string {
  let out: string[] = [];
  {
    let cb = () => q;
    let q = 1;
    out.push(String(cb()));
  }
  {
    let cb2 = () => q;
    let q = 2;
    out.push(String(cb2()));
  }
  return out.join(",");
}
function leak(): string {
  let out: string[] = [];
  out.push(String(q)); // before the block: outer binding, not TDZ
  {
    let cb = () => q;
    let q = 3;
    out.push(String(cb()));
  }
  out.push(String(q)); // after the block: outer binding again
  return out.join(",");
}
console.log(`${siblings()} ${leak()}`);
"#,
    );
    assert_eq!(stdout.trim(), "1,2 module,3,module");
}

/// Switch-case statement lists share the switch's block scope without being a
/// `BlockStmt`, so they need their own re-binding hook at lowering; a `var`
/// in a nested block hoists to function scope, so a closure in the ENCLOSING
/// scope must still capture the live (preallocated) box.
#[test]
fn switch_case_forward_capture_and_nested_var_hoist() {
    let stdout = compile_and_run(
        r#"
function sw(n: number): number {
  switch (n) {
    case 1: {
      let cb = () => q;
      let q = 10;
      return cb();
    }
    case 2:
      let cb2 = () => w;
      let w = 20;
      return cb2();
  }
  return -1;
}
function vh(): number {
  let cb = () => n;
  {
    var n = 5;
  }
  return cb();
}
console.log(`${sw(1)} ${sw(2)} ${vh()}`);
"#,
    );
    assert_eq!(stdout.trim(), "10 20 5");
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
