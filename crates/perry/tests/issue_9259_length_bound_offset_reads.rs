//! Regression coverage for #9259: an `arr.length`-bounded packed-f64 loop kept
//! its fast clone when the body read `a[k]`, but lost it **entirely** — not
//! partially — as soon as the body also read `a[k ± c]`.
//!
//! The failure was a cascade, not a missed element load. Three separate
//! predicates encode "an index this loop's guard covers" as a bare
//! `Expr::LocalGet(counter_id)`, so `a[k - 1]` (an `Expr::Binary`) matched
//! none of them. The matcher's body walker therefore declined
//! (`read_body_is_safe == false`, reported as `clone_not_call_free`), the read
//! fell back to a helper CALL, and the clone's call-free scan then discarded
//! the whole clone — taking the fast path for the plain `a[k]` with it. The
//! measured cost was 8 ms -> 72 ms on a 4096-element loop, flipping the shape
//! from beating node to 5.5x behind it.
//!
//! The fix admits a constant offset on the loop's own counter and pays the
//! same inline `icmp ult idx, len` a foreign counter already pays, taking the
//! fact's existing side exit when it fails. Reads only: a store side exit
//! re-executes the iteration, which is harmless for a read and would
//! double-apply a store.
//!
//! What these tests pin is the *admission*, not a timing: that the clone is
//! emitted at all for the offset body, and that the results still match the
//! generic path under a moving collector.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile(dir: &Path, source: &str) -> (PathBuf, String) {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .env("PERRY_NO_CACHE", "1")
        .env("PERRY_LLVM_KEEP_IR", "1")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    (
        output,
        String::from_utf8_lossy(&compile.stderr).into_owned(),
    )
}

/// The emitted IR, located the way `PERRY_LLVM_KEEP_IR` reports it.
fn kept_ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
}

fn packed_blocks(ir: &str) -> usize {
    ir.lines()
        .filter(|line| line.starts_with("packed_f64") && line.trim_end().ends_with(':'))
        .count()
}

fn run(bin: &Path, dir: &Path, moving_gc: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if moving_gc {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled binary")
}

fn assert_stdout(output: &Output, expected: &str, moving_gc: bool) {
    assert!(
        output.status.success(),
        "binary failed with moving_gc={moving_gc}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

/// `a[k]` alone under an `arr.length` bound — the shape that always worked.
/// Present as the positive control: without it, a regression that stopped
/// admitting *every* packed loop would leave the offset test below passing
/// vacuously in the other direction.
const PLAIN: &str = r#"
function run(a: number[]): number {
  let c = 0;
  for (let r = 0; r < 20; r++) {
    for (let k = 1; k < a.length; k++) {
      if (a[k] > 0.0) c++;
    }
  }
  return c;
}
const a: number[] = [];
for (let i = 0; i < 512; i++) a.push((i * 37) % 1000);
console.log(run(a));
"#;

/// The #9259 shape: same bound, same array, one constant-offset read added.
const OFFSET: &str = r#"
function run(a: number[]): number {
  let c = 0;
  for (let r = 0; r < 20; r++) {
    for (let k = 1; k < a.length; k++) {
      if (a[k] > a[k - 1]) c++;
    }
  }
  return c;
}
const a: number[] = [];
for (let i = 0; i < 512; i++) a.push((i * 37) % 1000);
console.log(run(a));
"#;

#[test]
fn length_bounded_loop_keeps_its_clone_when_the_body_reads_an_offset() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, plain_stderr) = compile(dir.path(), PLAIN);
    let plain = packed_blocks(&kept_ir(&plain_stderr));
    assert!(
        plain > 0,
        "positive control: the plain `a[k]` body must still get a packed clone, \
         otherwise the offset assertion below proves nothing"
    );

    let dir2 = tempfile::tempdir().expect("tempdir");
    let (_, offset_stderr) = compile(dir2.path(), OFFSET);
    let offset = packed_blocks(&kept_ir(&offset_stderr));
    assert!(
        offset > 0,
        "#9259: adding `a[k - 1]` to an `arr.length`-bounded body discarded the \
         ENTIRE packed clone (the offset read fell back to a helper call, and \
         the call-free scan then rejected the clone), so the plain `a[k]` in \
         the same loop lost its fast path too — 8ms -> 72ms"
    );
}

/// The offset read is bounds-checked against the live length and side-exits
/// rather than reading out of bounds, so the answer must match the generic
/// path — including when the collector is relocating underneath it.
#[test]
fn offset_reads_agree_with_the_generic_path_under_a_moving_collector() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), OFFSET);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "9860\n", moving_gc);
    }
}

/// `k - 1` is negative on the first iteration when the loop starts at 0. The
/// inline check is an UNSIGNED compare, so the negative index exceeds any
/// length and takes the side exit into the generic clone, which returns
/// `undefined` for the missing element exactly as the slow path does.
#[test]
fn a_negative_offset_index_side_exits_instead_of_reading_out_of_bounds() {
    let source = r#"
function run(a: number[]): number {
  let seen = 0;
  for (let k = 0; k < a.length; k++) {
    const prev = a[k - 1];
    if (prev === undefined) seen++;
  }
  return seen;
}
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(i * 1.5);
console.log(run(a));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "1\n", moving_gc);
    }
}
