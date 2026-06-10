//! Regression tests for the integer-locals provenance analysis
//! (`crates/perry-codegen/src/collectors/integer_locals.rs`).
//!
//! Bug class (#4785): a local wrongly kept in `integer_locals` gets an i32
//! shadow slot; when its f64 slot actually holds a NaN-boxed pointer, the
//! i32 read is `fptosi(NaN) = i32::MIN` and user code crashes with
//! `(number).method is not a function`. Each test compiles a TypeScript
//! source with the real binary and asserts the produced executable's stdout,
//! so any admission path that escapes transitive disqualification shows up
//! as a wrong value or a crash here.

use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &Path, source: &str) -> String {
    let entry = dir.join("main.ts");
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

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (signal/segfault = stale i32 shadow slot)\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

// The original #4785 shape: array destructuring emits a mutable
// `__destruct = undefined` scaffolding local; the init-only copy chain off it
// (`cb = v`) must not inherit a stale i32 slot when the scaffolding is
// disqualified by its non-integer writes.
#[test]
fn destructured_value_copy_is_not_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function build(entry: any): string {
  const [k, v] = entry;
  const cb = v;
  return k + ":" + cb.setName("users");
}
const builder = {
  setName(n: string): string { return "named-" + n; },
};
console.log(build(["col", builder]));
console.log(typeof build(["col", builder]));
"#,
    );
    assert_eq!(stdout, "col:named-users\nstring\n");
}

// A 2+ hop init-only copy chain off a disqualified `undefined` seed: every
// hop must be pruned, not just the first.
#[test]
fn multi_hop_copy_chain_off_disqualified_seed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function chain(flag: boolean): string {
  let a: any = undefined;
  if (flag) {
    a = { tag: () => "from-object" };
  }
  const b = a;
  const c = b;
  return c.tag();
}
console.log(chain(true));
"#,
    );
    assert_eq!(stdout, "from-object\n");
}

// Provenance through a clamp-style admission: `clamp3`-shaped helpers return
// one of their ARGUMENTS verbatim, so `const xx = clamp3(src, 0, 100)` is
// only integer when `src` is. With `src` holding an object, `xx` must not
// keep an i32 slot. (Pre-provenance, `is_int32_producing_expr` accepted any
// clamp call unconditionally and the init-only re-validation never pruned
// xx — this is the clamp_fn_ids bypass.)
#[test]
fn clamp_admitted_local_follows_disqualified_source() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function clamp3(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}
function pick(flag: boolean): string {
  let src: any = undefined;
  if (flag) {
    src = { name: () => "still-an-object" };
  }
  const xx = clamp3(src, 0, 100);
  const yy = xx;
  return yy.name();
}
console.log(pick(true));
console.log(clamp3(50, 0, 100), clamp3(-3, 0, 100), clamp3(7.5, 0, 5));
"#,
    );
    assert_eq!(stdout, "still-an-object\n50 0 5\n");
}

// A candidate WITH later integer writes must still be re-validated through
// its init: reads between `let b = a` (a disqualified) and the later `b = 1`
// must see the object, not a truncated i32.
#[test]
fn written_local_init_read_before_reassignment() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function probe(flag: boolean): string {
  const arr = [10, 20, 30];
  let a: any = undefined;
  if (flag) {
    a = { m: () => "object-alive" };
  }
  let b: any = a;
  const seen = b.m();
  b = 1;
  return seen + "/" + arr[b];
}
console.log(probe(true));
"#,
    );
    assert_eq!(stdout, "object-alive/20\n");
}

// Positive case: the optimization must still fire — a hot integer loop and a
// clamp-fed index chain still compute correct values. (Slot survival was
// eyeballed via `--trace llvm`; correctness is the hard gate here.)
#[test]
fn integer_loops_still_compute_correctly() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function hot(n: number): number {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    sum = sum + i;
  }
  return sum;
}
function clampIdx(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}
function kernel(): number {
  const data = [1, 2, 3, 4, 5, 6, 7, 8];
  const W = 8;
  const hi = W - 1;
  let acc = 0;
  for (let x = 0; x < W; x++) {
    const xx = clampIdx(x + 2, 0, hi);
    const idx = xx | 0;
    acc = acc + data[idx];
  }
  return acc;
}
console.log(hot(100000));
console.log(kernel());
"#,
    );
    // hot: 0+1+…+99999 = 4999950000 (exceeds i32 — must not truncate).
    // kernel: data[2..7] + data[7]*2 = 3+4+5+6+7+8+8+8 = 49.
    assert_eq!(stdout, "4999950000\n49\n");
}
