//! Regression tests for #5933: `await`/`yield` in loop CONDITION or UPDATE
//! position was evaluated once, before the loop, instead of on every
//! iteration.
//!
//! The async hoist's self-described "safe-but-incomplete approximation"
//! turned `while (await c())` into `let __a = await c(); while (__a) {…}` —
//! a truthy first value looped forever on stale state, a falsy one never
//! entered; async drain loops never re-awaited. do-while additionally
//! evaluated the condition BEFORE the first body run. The generator layer
//! had the sibling defect: `while (yield …)` cloned the condition (with the
//! embedded yield) into the linearizer's condition state, where codegen
//! lowers the residual yield to `0.0`.
//!
//! Fixed by restructuring at the hoist layer (async) and the linearize
//! layer (generators): condition → per-iteration body-top check; do-while
//! via the first-iteration flag (preserving `continue`-evaluates-condition
//! and body-before-first-condition order); for-update → body-end with
//! loop-level `continue`s prefixed by the update.
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

/// The canonical async drain loop: the awaited assignment-in-condition must
/// re-await every iteration (pre-fix: one shift, then an infinite or empty
/// loop depending on the first value).
#[test]
fn while_await_assignment_condition_drains() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function drain() {
  const q = [1, 2, 3];
  const out: number[] = [];
  let v: any;
  while ((v = await Promise.resolve(q.shift())) !== undefined) {
    out.push(v);
  }
  return out.join(",");
}
drain().then((r) => console.log("drain:", r));
"#,
    );
    assert_eq!(stdout, "drain: 1,2,3\n");
}

/// The awaited condition function must be CALLED once per iteration plus
/// the terminating call — counts prove per-iteration evaluation.
#[test]
fn while_await_condition_reevaluates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let n = 0;
async function next(): Promise<boolean> {
  n++;
  return n < 4;
}
async function run() {
  let ticks = 0;
  while (await next()) {
    ticks++;
  }
  console.log("ticks:", ticks, "calls:", n);
}
run();
"#,
    );
    assert_eq!(stdout, "ticks: 3 calls: 4\n");
}

/// do-while with an awaited condition: the body runs BEFORE the first
/// condition evaluation, and `continue` must evaluate the condition (the
/// c===0 continue would loop forever if it skipped it).
#[test]
fn do_while_await_condition_and_continue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let c = 0;
async function cond(): Promise<boolean> {
  c++;
  return c < 3;
}
async function run() {
  const out: string[] = [];
  do {
    out.push("body" + c);
    if (c === 0) continue;
  } while (await cond());
  console.log(out.join(","), "conds:", c);
}
run();
"#,
    );
    assert_eq!(stdout, "body0,body1,body2 conds: 3\n");
}

/// for-loop with an awaited condition.
#[test]
fn for_await_condition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function limit(i: number): Promise<boolean> {
  return i < 3;
}
async function run() {
  const out: number[] = [];
  for (let i = 0; await limit(i); i++) {
    out.push(i);
  }
  console.log("for:", out.join(","));
}
run();
"#,
    );
    assert_eq!(stdout, "for: 0,1,2\n");
}

/// for-loop with an awaited UPDATE and a `continue`: continue must run the
/// awaited update before re-testing (node: 0,4,6 — the i===2 continue still
/// bumps i via the awaited update).
#[test]
fn for_await_update_with_continue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function bump(i: number): Promise<number> {
  return i + 2;
}
async function run() {
  const out: number[] = [];
  for (let i = 0; i < 8; i = await bump(i)) {
    if (i === 2) continue;
    out.push(i);
  }
  console.log("upd:", out.join(","));
}
run();
"#,
    );
    assert_eq!(stdout, "upd: 0,4,6\n");
}

/// Direct generator: `while ((got = yield …) !== "stop")` — the yield in
/// the condition must suspend per iteration (pre-fix the residual yield in
/// the cloned condition lowered to 0.0).
#[test]
fn generator_yield_in_while_condition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* g() {
  let got: any;
  const out: any[] = [];
  while ((got = yield out.length) !== "stop") {
    out.push(got);
  }
  return out.join("");
}
const it = g();
console.log(it.next().value);
console.log(it.next("a").value);
console.log(it.next("b").value);
console.log(JSON.stringify(it.next("stop")));
"#,
    );
    assert_eq!(stdout, "0\n1\n2\n{\"value\":\"ab\",\"done\":true}\n");
}

/// Short-circuit: the await sits in the RIGHT operand of `&&` in the
/// condition — evaluated per iteration only when the left side is truthy.
#[test]
fn while_logical_right_await_condition() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
let calls = 0;
async function gate(): Promise<boolean> {
  calls++;
  return calls < 3;
}
async function run(flagStart: boolean) {
  let flag = flagStart;
  let spins = 0;
  while (flag && (await gate())) {
    spins++;
  }
  console.log("spins:", spins, "calls:", calls);
}
run(true);
"#,
    );
    assert_eq!(stdout, "spins: 2 calls: 3\n");
}

/// `continue` inside a `try` WITHOUT `finally`, with an awaited update: the
/// prefix applies inside the try (no abrupt-completion ordering concern) and
/// the update still runs on the continue path. (With a `finally` present the
/// transform deliberately bails to the previous lowering — a `continue`
/// there must run the finally BEFORE the update, and the finally may
/// override it; see stmts_have_continue_inside_try_finally.)
#[test]
fn for_await_update_continue_inside_try_no_finally() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function bump(i: number): Promise<number> {
  return i + 1;
}
async function run() {
  const out: string[] = [];
  for (let i = 0; i < 4; i = await bump(i)) {
    try {
      if (i === 1) continue;
      out.push("v" + i);
    } catch (e) {
      out.push("err");
    }
  }
  console.log(out.join(","));
}
run();
"#,
    );
    assert_eq!(stdout, "v0,v2,v3\n");
}
