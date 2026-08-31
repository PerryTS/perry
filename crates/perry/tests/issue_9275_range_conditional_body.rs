//! Regression coverage for #9275: the packed-f64 RANGE tier rejected any
//! `if (...) counter++` body, so a conditional-count loop got a fast clone
//! only when its bound was spelled `arr.length`.
//!
//! The versioned tier already accepts the shape — `expr_is_packed_f64_loop_safe`
//! recurses through `Expr::Compare`, and integer `c++` accumulators admit
//! independently — so the identical body was 4 ms with an `arr.length` bound
//! and 31 ms with a literal one, which is the difference between beating node
//! by 2.5x and losing to it by 3.1x.
//!
//! The fix adds a `Stmt::If` arm to the DENSE range walk, and dense is the
//! mode where it belongs: its loads have no side exits, so an iteration runs
//! entirely in the fast copy or entirely in the slow one and a branch cannot
//! leave a half-applied iteration behind. The classic range mode cannot take
//! this — it permits a hole-read side exit that re-executes the iteration,
//! which is exactly why it insists on a single statement whose one side effect
//! happens last.
//!
//! These tests pin the admission and the answers, not a timing.

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

fn packed_blocks(stderr: &str) -> usize {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path)
        .expect("read kept LLVM IR")
        .lines()
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

fn source(body: &str) -> String {
    format!(
        r#"
function run(a: number[]): number {{
  let c = 0.0;
  for (let r = 0; r < 20; r++) {{
    for (let k = 1; k < 512; k++) {{
      {body}
    }}
  }}
  return c;
}}
const a: number[] = [];
for (let i = 0; i < 512; i++) a.push((i * 37) % 100);
console.log(run(a));
"#
    )
}

/// The literal-bounded accumulate body always got a clone. It is the positive
/// control: it shares the bound with the conditional cases below, so a zero
/// there is attributable to the body shape and not to the bound.
#[test]
fn the_literal_bound_accumulate_body_still_gets_a_clone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, stderr) = compile(dir.path(), &source("c += a[k];"));
    assert!(
        packed_blocks(&stderr) > 0,
        "positive control: a literal-bounded `c += a[k]` must keep its packed clone"
    );
}

#[test]
fn a_literal_bounded_conditional_count_gets_a_clone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, stderr) = compile(dir.path(), &source("if (a[k] > 50.0) c++;"));
    assert!(
        packed_blocks(&stderr) > 0,
        "#9275: the range tier rejected any `if (...) c++` body, so this loop got no \
         packed clone at all while the identical body with an `arr.length` bound did \
         — 31ms against 4ms"
    );
}

/// The offset form too: the conditional is what was rejected, not the index.
#[test]
fn a_literal_bounded_conditional_over_an_offset_read_gets_a_clone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (_, stderr) = compile(dir.path(), &source("if (a[k] > a[k - 1]) c++;"));
    assert!(
        packed_blocks(&stderr) > 0,
        "#9275: offset form of the same shape"
    );
}

/// An `if`/`else` where both branches write. The condition carries an offset
/// read, so the entry guard validates the whole window the statement touches.
#[test]
fn both_branches_are_admitted_and_agree_with_the_generic_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = source("if (a[k] > a[k - 1]) { c++; } else { c--; }");
    let (bin, stderr) = compile(dir.path(), &src);
    assert!(
        packed_blocks(&stderr) > 0,
        "if/else body should be admitted"
    );
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "2660\n", moving_gc);
    }
}

/// The boundary of this change, pinned deliberately.
///
/// A float accumulator whose RHS reads a tracked array (`c += a[k]`) is NOT
/// admitted by the dense walk — and was not before this change either, which
/// is checkable without an `if` at all: a plain two-statement body
/// `c += a[k]; c += 1.0;` is rejected by dense mode on `main` and still is.
/// The reason is not the conditional: `c + a[k]` can lower to a dynamic add,
/// which is a collecting call, so the accumulator needs a numeric proof this
/// walk does not have. That proof lives in the accumulator-admission path and
/// is being widened separately.
///
/// This test exists so the limitation is recorded as a known boundary rather
/// than rediscovered as a bug, and so that whoever widens the accumulator
/// proof sees a case that should start getting a clone. The answer must be
/// correct either way, via the generic path.
#[test]
fn a_float_accumulator_reading_the_array_is_not_admitted_but_is_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = source("if (a[k] > 50.0) { c += a[k]; }");
    let (bin, _) = compile(dir.path(), &src);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "375180\n", moving_gc);
    }
}

/// A `break` inside the branch is deliberately NOT admitted: dense mode's
/// guarantee is that a whole iteration runs in one copy, and an early exit out
/// of a branch is a shape the walk has not reasoned about. It must still
/// compile and produce the right answer via the generic path.
#[test]
fn a_break_inside_the_branch_stays_on_the_generic_path_and_is_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = source("if (a[k] > 98.0) { break; } c += 1.0;");
    let (bin, _) = compile(dir.path(), &src);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "520\n", moving_gc);
    }
}
