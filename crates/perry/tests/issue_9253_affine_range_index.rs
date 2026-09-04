//! Regression coverage for #9248 and its affine-index implementation #9253:
//! a counted loop whose reads use AFFINE indices — `a[i * size + k]` and
//! `b[k * size + j]` — guards both receivers in the preheader and keeps the
//! loop-carried accumulator native.
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
//! shape and multi-array accumulator admission — a loop-invariant
//! local/parameter bound, N-array guard emission, and GC-safe receiver caching
//! refreshed at the back-edge poll. This adds the
//! affine index: the entry guard proves the RECEIVER once, and each read pays
//! one inline `icmp ult idx, len` with a side exit.
//!
//! Endpoint window hoisting and admitting the guarded array set for the
//! accumulator then reduced the benchmark from 69 ms to 16 ms against node's
//! 32 ms (the original baseline was 100 ms).

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

fn specialized_function_ir<'a>(text: &'a str, function: &str) -> &'a str {
    let marker = format!("__{function}$spec");
    let marker_at = text
        .find(&marker)
        .unwrap_or_else(|| panic!("missing specialized function `{function}`"));
    let start = text[..marker_at]
        .rfind("define ")
        .unwrap_or_else(|| panic!("missing definition for specialized function `{function}`"));
    let rest = &text[start..];
    let end = rest
        .find("\n}\n")
        .unwrap_or_else(|| panic!("unterminated specialized function `{function}`"));
    &rest[..end + 2]
}

fn named_block<'a>(function_ir: &'a str, prefix: &str) -> &'a str {
    let marker = format!("\n{prefix}");
    let start = function_ir
        .find(&marker)
        .unwrap_or_else(|| panic!("missing block `{prefix}`"))
        + 1;
    let rest = &function_ir[start..];
    let end = rest.find("\n\n").unwrap_or(rest.len());
    &rest[..end]
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

/// Both affine receiver guards reach the clone and the accumulator remains a
/// native double. Losing either property restores #9248's per-read guard or
/// per-iteration dynamic-add cost.
#[test]
fn matmul_guards_both_affine_arrays_and_unboxes_the_accumulator() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), MATMUL);
    let text = ir(&stderr);
    // The admission signal: the 2-arg receiver-only guard is emitted only by
    // the affine arm (the versioned tier also uses this symbol, but it cannot
    // fire here — the bound is a parameter, not `arr.length`). The old
    // detector looked for the per-read `packed_f64_affine.index_fits` block,
    // which the window-hoist legitimately removed: a window proven at the
    // loop's endpoints leaves the read as a bare trunc + raw load with no
    // named block at all. Count CALLS (not the declaration): one must guard
    // `a` and one must guard `b`.
    let matmul_ir = specialized_function_ir(&text, "matmul");
    assert_eq!(
        matmul_ir
            .matches("call i32 @js_typed_feedback_packed_f64_array_loop_guard")
            .count(),
        2,
        "#9248: both affine array receivers must be guarded once in the preheader"
    );

    // The fast body's two raw loads feed one native multiply and add. In
    // particular, the loop must not regain either generic index helpers or
    // the boxed accumulator's dynamic add / write-barrier branch.
    let fast_body = named_block(matmul_ir, "for.packed_f64_range_fast.body.");
    assert!(
        fast_body.matches("load double, ptr").count() >= 3,
        "#9248: expected accumulator plus two raw f64 loads:\n{fast_body}"
    );
    for instruction in ["fmul double", "fadd double", "store double"] {
        assert!(
            fast_body.contains(instruction),
            "#9248: missing `{instruction}` in the packed fast body:\n{fast_body}"
        );
    }
    for fallback in [
        "@js_typed_feedback_array_index_get_fallback_boxed",
        "@js_dynamic_string_or_number_add",
        "shadow.root.barrier",
    ] {
        assert!(
            !fast_body.contains(fallback),
            "#9248: packed fast body regained `{fallback}`:\n{fast_body}"
        );
    }
    for moving_gc in [false, true] {
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "matmul:2983\n",
            moving_gc,
        );
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
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "overrun:NaN\n",
            moving_gc,
        );
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

/// The #9294 guard arm took its receiver-only `continue` for arrays with
/// BOTH counter-offset and affine accesses, skipping the windowed guard while
/// the counter fact still said `window_validated: true` — `a[k + 1]` then
/// read one raw slot past the loop's window at the boundary. Mixed arrays
/// now fall through to the windowed guard. Node's answer: the last
/// iteration's `a[k + 1]` is `a[size]`, out of bounds, `undefined`, NaN.
#[test]
fn a_mixed_offset_and_affine_array_validates_its_counter_window() {
    let source = r#"
function run(a: number[], size: number): number {
  let s = 0.0;
  for (let i = 0; i < 1; i++) {
    for (let k = 0; k < size; k++) {
      s = s * 1.0 + a[k + 1] + a[i * size + k];
    }
  }
  return s;
}
const a: number[] = [];
for (let i = 0; i < 64; i++) a.push(1.0);
console.log("s:" + run(a, 64));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "s:NaN\n", moving_gc);
    }
}
