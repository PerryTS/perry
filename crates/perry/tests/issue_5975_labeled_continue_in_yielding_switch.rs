//! Regression test for #5975: a `continue <label>` inside a **yielding**
//! `switch` case nested in a labeled loop spun forever.
//!
//! `loop: while (true) { switch (x) { case …: yield …; continue loop } break loop }`
//! desugars the switch (#5868) into a guarded `if`-chain and splits it at the
//! `yield`, but the labeled-loop linearizer's `rewrite_labeled_bc_in_stmts`
//! never descended into the nested switch, so the `continue loop`
//! (`LabeledContinue`) survived verbatim into the state machine. Nothing
//! lowered it, the loop never re-entered, and the generator spun at 100% CPU
//! and never produced a value — most visibly the `yaml` package's block-scalar
//! / indicator lexer generators (`loop: while (true) { switch (this.charAt(0))
//! { … continue loop } }`), which hung any `parse()` of a large minified
//! bundle at module-init time.
//!
//! The unlabeled equivalent (`continue;`) always worked, because
//! `switch_cases_have_loop_continue` detects a plain `Stmt::Continue`; only the
//! labeled form was invisible to it. The fix rewrites `continue <label>` →
//! plain `continue` when descending into a nested switch (a switch never
//! captures `continue`), matching the unlabeled path.
//!
//! Expected outputs are byte-for-byte what `node --experimental-strip-types`
//! prints. Each run is bounded by a timeout so a regression FAILS (with a
//! spin) instead of hanging the test process.

use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// Compile `source`, run the binary with a wall-clock deadline, and return its
/// stdout. Panics (fails the test) if compilation fails, the binary exits
/// non-zero, or it does not finish within `timeout` (the pre-fix spin).
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

    let mut child = Command::new(&output)
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn compiled binary");

    let timeout = Duration::from_secs(30);
    let start = Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let out = child.wait_with_output().expect("wait_with_output");
                assert!(
                    status.success(),
                    "compiled binary failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
                    status.code(),
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                return String::from_utf8_lossy(&out.stdout).into_owned();
            }
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "compiled binary did not finish within {:?} — the #5975 \
                         labeled-continue-in-yielding-switch spin regressed",
                        timeout
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// The minimal reproducer: a generator with a labeled `while (true)` whose
/// yielding switch cases `continue loop`, and a `break loop` after the switch.
#[test]
fn generator_labeled_continue_in_yielding_switch_while() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* gen(input: string) {
  let i = 0, n = 0;
  loop: while (true) {
    switch (input[i]) {
      case 'a': yield 'A'; i++; n++; continue loop;
      case 'b': yield 'B'; i++; n++; continue loop;
    }
    break loop;
  }
  return n;
}
const out: string[] = [];
const g = gen("aabbc");
let r = g.next();
while (!r.done) { out.push(r.value as string); r = g.next(); }
console.log(out.join(",") + "|n=" + r.value);
"#,
    );
    assert_eq!(stdout, "A,A,B,B|n=4\n");
}

/// Same shape with a labeled `for (;;)` loop.
#[test]
fn generator_labeled_continue_in_yielding_switch_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* gen(input: string) {
  let n = 0;
  loop: for (let i = 0; ; ) {
    switch (input[i]) {
      case 'a': yield 'A'; i++; n++; continue loop;
      case 'b': yield 'B'; i++; n++; continue loop;
    }
    break loop;
  }
  return n;
}
const out: string[] = [];
const g = gen("abbc");
let r = g.next();
while (!r.done) { out.push(r.value as string); r = g.next(); }
console.log(out.join(",") + "|n=" + r.value);
"#,
    );
    assert_eq!(stdout, "A,B,B|n=3\n");
}

/// The async equivalent: `await` inside the switch case, then `continue loop`.
#[test]
fn async_labeled_continue_in_awaiting_switch_while() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
async function f(input: string) {
  let i = 0, n = 0, out = "";
  loop: while (true) {
    switch (input[i]) {
      case 'a': out += await Promise.resolve('A'); i++; n++; continue loop;
      case 'b': out += await Promise.resolve('B'); i++; n++; continue loop;
    }
    break loop;
  }
  return out + "|n=" + n;
}
f("aabbc").then((v) => console.log(v));
"#,
    );
    assert_eq!(stdout, "AABB|n=4\n");
}

/// A closer analogue of the `yaml` indicator lexer: the loop-continuing cases
/// `yield*` a delegated generator before `continue loop`, and a `break loop`
/// terminates when no case matches.
#[test]
fn generator_yield_delegate_then_labeled_continue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function* emit(tag: string, k: number) {
  for (let j = 0; j < k; j++) yield tag;
}
function* lex(src: string) {
  let i = 0, count = 0;
  loop: while (true) {
    switch (src[i]) {
      case '!':
        count += yield* emit("bang", 1);
        i++;
        continue loop;
      case '&':
        count += yield* emit("amp", 2);
        i++;
        continue loop;
    }
    break loop;
  }
  return count;
}
const out: string[] = [];
const g = lex("!&!x");
let r = g.next();
while (!r.done) { out.push(r.value as string); r = g.next(); }
console.log(out.join(",") + "|count=" + r.value);
"#,
    );
    // `emit` yields the tag string; `count += yield* emit(...)` adds the
    // generator's return value (undefined -> NaN in JS), matching node.
    assert_eq!(stdout, "bang,amp,amp,bang|count=NaN\n");
}
