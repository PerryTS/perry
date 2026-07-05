//! Regression tests for #5868: `switch` inside async/generator state
//! machines miscompiled.
//!
//! The linearizer had no `Stmt::Switch` arm, so a switch whose case body
//! contained a `yield`/`await` fell to the catch-all, was emitted unsplit
//! inside one state, and codegen lowered the embedded residual `Expr::Yield`
//! to `0.0` — `async f(x){ switch(x){ case 1: return await g() } }`
//! resolved to `0` without suspending, and a generator `yield` inside a
//! case vanished. Separately, a loop-level `continue` inside a (yield-free)
//! switch in a CPS'd loop survived as a raw `Stmt::Continue` the dispatch
//! loop ignored.
//!
//! Both now route through `desugar_switch_to_ifs` (match-index + guarded
//! `if` chain), which preserves JS switch semantics: discriminant evaluated
//! once, case tests evaluated in order only until the first match,
//! fallthrough, default-in-the-middle, `break` (including from nested
//! `if`/`try`), and loop-level `continue`.
//!
//! All expected outputs are byte-for-byte what `node
//! --experimental-strip-types` prints.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
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
        "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

/// Issue repro 1: `return await` inside a case resolved to `0`.
#[test]
fn return_await_inside_case() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function pick(x: number) {
  switch (x) {
    case 1:
      return await Promise.resolve("one");
  }
  return "other";
}
pick(1).then((v) => console.log("got:", v));
"#,
    );
    assert_eq!(stdout, "got: one\n");
}

/// Issue repro 2: assignment-form await inside a case resolved to `0`.
#[test]
fn assign_await_inside_case() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function pick2(x: number) {
  let out = "other";
  switch (x) {
    case 1: {
      out = await Promise.resolve("uno");
      break;
    }
  }
  return out;
}
pick2(1).then((v) => console.log("got2:", v));
"#,
    );
    assert_eq!(stdout, "got2: uno\n");
}

/// Issue repro 3: a generator `yield` inside a case vanished.
#[test]
fn generator_yield_inside_case() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* g(x: number) {
  switch (x) {
    case 1:
      yield "a";
  }
  yield "b";
}
console.log([...g(1)].join(","));
"#,
    );
    assert_eq!(stdout, "a,b\n");
}

/// Issue repro 4: a loop-level `continue` inside a yield-free switch in a
/// CPS'd loop was silently ignored (the iteration's remainder ran anyway).
#[test]
fn continue_inside_switch_in_async_loop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function loop() {
  let i = 0;
  let hits = 0;
  while (i < 3) {
    i++;
    await Promise.resolve();
    switch (i) {
      case 1:
        continue;
    }
    hits++;
  }
  return hits;
}
loop().then((h) => console.log("hits:", h));
"#,
    );
    assert_eq!(stdout, "hits: 2\n");
}

/// Fallthrough across awaits: matched case falls into the next case's body
/// until the `break`.
#[test]
fn fallthrough_across_awaits() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function run() {
  const out: string[] = [];
  switch (1) {
    case 1:
      out.push(await Promise.resolve("A"));
    case 2:
      out.push(await Promise.resolve("B"));
      break;
    case 3:
      out.push("C");
  }
  return out.join("");
}
run().then((v) => console.log("fall:", v));
"#,
    );
    assert_eq!(stdout, "fall: AB\n");
}

/// Default in the MIDDLE with no matching case: execution starts at the
/// default clause and falls through the clauses after it.
#[test]
fn default_in_the_middle_no_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function run(x: number) {
  const out: string[] = [];
  switch (x) {
    case 1:
      out.push(await Promise.resolve("one"));
      break;
    default:
      out.push(await Promise.resolve("dflt"));
    case 2:
      out.push(await Promise.resolve("two"));
      break;
    case 3:
      out.push("three");
  }
  return out.join(",");
}
run(9).then((v) => console.log("mid-default:", v));
"#,
    );
    assert_eq!(stdout, "mid-default: dflt,two\n");
}

/// Case tests are evaluated in order, exactly once each, and only until the
/// first match — a side-effecting test after the match must NOT run.
#[test]
fn case_tests_evaluate_in_order_until_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function t(n: number): number {
  console.log("test", n);
  return n;
}
async function run() {
  switch (2) {
    case t(1):
      console.log("b1");
    case t(2):
      console.log("b2");
      await Promise.resolve();
      console.log("b2b");
      break;
    case t(3):
      console.log("b3");
  }
  console.log("after");
}
run();
"#,
    );
    assert_eq!(stdout, "test 1\ntest 2\nb2\nb2b\nafter\n");
}

/// `break` from inside an `if` in a case body must abort the rest of the
/// case AND the fallthrough; without the break, both continue.
#[test]
fn break_inside_if_in_case_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function run(c: boolean) {
  const out: string[] = [];
  switch (1) {
    case 1:
      out.push(await Promise.resolve("x"));
      if (c) break;
      out.push("more");
    case 2:
      out.push("fall");
  }
  return out.join(",");
}
run(true).then((v) => console.log("brk-if-t:", v));
run(false).then((v) => console.log("brk-if-f:", v));
"#,
    );
    assert_eq!(stdout, "brk-if-t: x\nbrk-if-f: x,more,fall\n");
}

/// A LABELED switch with `break label` from a case body containing an
/// await: the labeled break is the switch's own break.
#[test]
fn labeled_switch_break_label() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function run() {
  l: switch (1) {
    case 1:
      console.log(await Promise.resolve("in-case"));
      break l;
    case 2:
      console.log("no");
  }
  console.log("after");
}
run();
"#,
    );
    assert_eq!(stdout, "in-case\nafter\n");
}
