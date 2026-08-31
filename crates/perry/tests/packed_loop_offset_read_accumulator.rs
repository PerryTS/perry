//! Offset array reads (`a[i ± c]`) earn the packed clone's numeric proof
//! (#9259 follow-up).
//!
//! `accumulator_rhs_is_numeric` required a bare `Expr::LocalGet` index, so
//! `a[k - 1]` was not numeric, the accumulator never earned its number proof,
//! and every `+` in the enclosing expression lowered to a tag-test diamond
//! over `js_dynamic_string_or_number_add`. That cost 41 ms against 8 ms for
//! the same loop without the offset — the shape #9060 and #9091 already fixed
//! for the bare-counter form.
//!
//! **What these tests guard is soundness, not speed.** The perf is a
//! benchmark's job; the risk this change introduces is admitting a read as
//! numeric when the tier does not actually lower it inline. The admission is
//! threaded per tier (`offset_reads_inlined`): the range tier publishes
//! `window_validated` and hole-checks its loads, so an offset read yields a
//! Number; the versioned and stable-packed tiers pass `false`, because their
//! offset reads take the generic path and can produce `undefined`.
//!
//! Every case below puts a value the fast path must NOT treat as a raw double
//! inside the window — an out-of-range index, a hole, a non-numeric element,
//! a string that must concatenate rather than add. If someone later flips the
//! flag on a tier that does not inline offset reads, these are what break.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

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
        .env("PERRY_NO_CACHE", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run: Output = Command::new(&output)
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
    String::from_utf8_lossy(&run.stdout).trim().to_owned()
}

const PRELUDE: &str = r#"
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(i);
"#;

#[test]
fn an_offset_read_sums_correctly() {
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): number {{
  let s = 0;
  for (let k = 1; k < 64; k++) s = s + a[k] + a[k - 1];
  return s;
}}
console.log(f());
"#
    ));
    assert_eq!(out, "3969");
}

#[test]
fn a_negative_index_at_the_window_start_is_not_a_number() {
    // k = 0 reads a[-1]. If the offset read were admitted as numeric on a tier
    // that does not lower it inline, the generic read's `undefined` would be
    // consumed as a raw double instead of poisoning the sum to NaN.
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): string {{
  let s = 0;
  for (let k = 0; k < 64; k++) s = s + a[k - 1];
  return "neg:" + s;
}}
console.log(f());
"#
    ));
    assert_eq!(out, "neg:NaN");
}

#[test]
fn an_offset_running_past_the_end_is_not_a_number() {
    let out = compile_and_run(&format!(
        r#"{PRELUDE}
function f(): string {{
  let s = 0;
  for (let k = 0; k < 64; k++) s = s + a[k + 8];
  return "past:" + s;
}}
console.log(f());
"#
    ));
    assert_eq!(out, "past:NaN");
}

#[test]
fn a_hole_inside_the_offset_window_is_not_a_number() {
    let out = compile_and_run(
        r#"
function f(): string {
  const h: number[] = [1,2,3,4,5,6,7,8];
  delete h[3];
  let s = 0;
  for (let k = 1; k < 8; k++) s = s + h[k - 1];
  return "hole:" + s;
}
console.log(f());
"#,
    );
    assert_eq!(out, "hole:NaN");
}

#[test]
fn a_non_numeric_element_inside_the_window_still_coerces() {
    let out = compile_and_run(
        r#"
function f(): string {
  const m: any[] = [1,2,"x",4,5,6,7,8];
  let s = 0;
  for (let k = 1; k < 8; k++) s = s + m[k - 1];
  return "mixed:" + s;
}
console.log(f());
"#,
    );
    assert_eq!(out, "mixed:3x4567");
}

#[test]
fn a_leading_string_element_concatenates_rather_than_adds() {
    // The sharpest of these: a wrongly-admitted numeric proof turns `+` into a
    // native `fadd`, which cannot concatenate. Node produces a string here.
    let out = compile_and_run(
        r#"
function f(): string {
  const m: any[] = ["a",1,2,3,4,5,6,7];
  let s: any = 0;
  for (let k = 1; k < 8; k++) s = s + m[k - 1];
  return "cat:" + s;
}
console.log(f());
"#,
    );
    assert_eq!(out, "cat:0a123456");
}
