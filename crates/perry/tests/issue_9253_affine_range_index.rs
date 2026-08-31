//! Regression coverage for #9253: a counted loop whose reads use an AFFINE
//! index — `a[i * size + k]` — now hoists its receiver guard into the
//! preheader instead of re-deriving it on every access.
//!
//! `16_matrix_multiply`'s inner loop spends 97% of its time inside generated
//! code with no runtime calls, so the cost was never a missed inlining. Per
//! iteration, for BOTH receivers, it re-derived the pointer tag check, the
//! handle-band check, the header dereference, the `_reserved` flag tests and
//! the two 16,000,000 length/capacity sanity compares — for receivers that are
//! loop-invariant parameters whose headers cannot change inside the loop.
//! LLVM cannot hoist any of it: the guard reloads the header through a pointer
//! it cannot prove unaliased, and the incremental-barrier atomic read is a
//! motion barrier.
//!
//! The packed-f64 RANGE tier already had everything needed except the index
//! shape — a loop-invariant local/parameter bound, N-array guard emission, and
//! GC-safe receiver caching refreshed at the back-edge poll. This adds the
//! affine index: the entry guard proves the RECEIVER once, and each read pays
//! one inline `icmp ult idx, len` with a side exit.
//!
//! Measured on an idle Mac mini, self-timed min of 7: 100 ms -> 69 ms against
//! node's 33 ms (3.03x -> 2.09x).

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

fn ir(stderr: &str) -> String {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path).expect("read kept LLVM IR")
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

const MATMUL: &str = r#"
function matmul(a: number[], b: number[], size: number): number {
  let acc = 0.0;
  for (let i = 0; i < size; i++)
    for (let j = 0; j < size; j++) {
      let sum = 0.0;
      for (let k = 0; k < size; k++) sum = sum + a[i * size + k] * b[k * size + j];
      acc = acc + sum;
    }
  return acc;
}
const a: number[] = []; const b: number[] = [];
for (let i = 0; i < 64; i++) { a.push(i % 7); b.push(i % 5); }
console.log("matmul:" + matmul(a, b, 8));
"#;

/// The affine index shape reaches the clone at all. Without this the guard is
/// re-derived per access for both receivers, which is the whole issue.
#[test]
fn an_affine_index_admits_the_range_clone_and_agrees_with_the_generic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), MATMUL);
    let text = ir(&stderr);
    assert!(
        text.contains("packed_f64_affine"),
        "#9253: `a[i * size + k]` must reach the affine read lowering; without it \
         the receiver guard re-executes per access"
    );
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "matmul:2983\n", moving_gc);
    }
}

/// An affine index that runs past the end must take the per-read side exit and
/// produce node's `undefined` semantics, not a raw out-of-bounds load. This is
/// the assertion that the bounds check is real: the entry guard validated the
/// receiver, NOT any index window, so nothing else stands between the affine
/// index and the element storage.
#[test]
fn an_affine_index_past_the_end_side_exits_to_undefined() {
    let source = r#"
function overrun(a: number[], size: number): number {
  let s = 0.0;
  for (let k = 0; k < size; k++) s = s + a[k * 3 + 2];
  return s;
}
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(i % 7);
console.log("overrun:" + overrun(a, 40));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "overrun:NaN\n", moving_gc);
    }
}

/// A negative affine index. The inline check is an UNSIGNED compare, so a
/// negative value reads as a huge unsigned one and exits rather than indexing
/// backwards out of the element storage.
#[test]
fn a_negative_affine_index_side_exits_instead_of_reading_backwards() {
    let source = r#"
function negative(a: number[], size: number): number {
  let s = 0.0;
  for (let k = 0; k < size; k++) { const v = a[k - 3]; if (v === undefined) s = s + 1.0; }
  return s;
}
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(i % 7);
console.log("negative:" + negative(a, 20));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "negative:3\n", moving_gc);
    }
}
