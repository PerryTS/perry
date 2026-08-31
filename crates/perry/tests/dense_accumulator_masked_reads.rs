//! A float accumulator over masked reads earns the dense range clone.
//!
//! `sum = sum * x[i & 63] + x[(i * 7) & 63]` (the `17_loop_data_dependent`
//! shape) was rejected by the dense range tier while `sum = sum * x[i & 63]`
//! was admitted — the discriminator was the accumulator's static numeric
//! proof. `+` can be concatenation, so the per-statement proof demands both
//! operands numeric; a reassigned accumulator has no such proof, because its
//! own writes read the guarded array, whose element proof only exists once
//! the guard has run. Chicken-and-egg, broken previously only for `*`.
//!
//! The matcher now peels the accumulator: it retries the proof with the
//! `LocalSet` target treated as numeric BY CONTRACT, then verifies every such
//! pending local with the same collector the lowering runs. The contract is
//! enforced at run time twice over: the clone's entry emits a genuine-double
//! tag check on the accumulator (a string-seeded accumulator routes to the
//! slow copy), and the dense entry guard validates the whole masked window
//! hole-free (a string ELEMENT fails the guard the same way).
//!
//! Measured: 17_loop_data_dependent 505 ms -> 227 ms against node's 229 ms.

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

fn fast_copies(stderr: &str) -> usize {
    let path = stderr
        .lines()
        .find_map(|line| line.split("kept LLVM IR: ").nth(1))
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("PERRY_LLVM_KEEP_IR did not report an IR path\n{stderr}"));
    std::fs::read_to_string(path)
        .expect("read kept LLVM IR")
        .lines()
        .filter(|l| l.starts_with("packed_f64_range") && l.trim_end().ends_with(':'))
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

/// The data-dependent recurrence gets a dense fast copy, and its result is
/// bit-identical to node's across 200k iterations of float churn.
#[test]
fn a_masked_read_accumulator_earns_the_dense_clone_and_matches_node() {
    let source = r#"
function run(x: number[]): number {
  let sum = 1.0;
  for (let i = 0; i < 200000; i++) sum = sum * x[i & 63] + x[(i * 7) & 63];
  return sum;
}
const x: number[] = [];
for (let i = 0; i < 64; i++) x.push(0.5 + i * 0.01);
console.log("r:" + run(x));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), source);
    assert!(
        fast_copies(&stderr) > 0,
        "the accumulator peel must admit the dense clone; without it the whole \
         loop pays the per-access guard tier (505ms vs node's 229ms)"
    );
    for moving_gc in [false, true] {
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "r:44.18806624016606\n",
            moving_gc,
        );
    }
}

/// The contract's first enforcement point: an accumulator seeded with a
/// STRING must take the entry tag check into the slow copy and produce node's
/// concatenation, not a raw fadd over a string box.
#[test]
fn a_string_seeded_accumulator_takes_the_slow_copy_and_concatenates() {
    let source = r#"
function run(x: number[]): string {
  let sum: any = "s";
  for (let i = 0; i < 4; i++) sum = sum + x[i & 63];
  return sum;
}
const x: number[] = [];
for (let i = 0; i < 64; i++) x.push(0.5 + i * 0.01);
console.log("r:" + run(x));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "r:s0.50.510.520.53\n",
            moving_gc,
        );
    }
}

/// The second enforcement point: a string ELEMENT makes the dense entry guard
/// fail (the window is not hole-free numeric), so the slow copy concatenates
/// exactly as node does.
#[test]
fn a_string_element_fails_the_guard_and_the_slow_copy_matches_node() {
    let source = r#"
function run(x: any[]): any {
  let sum: any = 0.0;
  for (let i = 0; i < 4; i++) sum = sum + x[i & 63];
  return sum;
}
const y: any[] = [];
for (let i = 0; i < 64; i++) y.push(i === 2 ? "boom" : i * 1.0);
console.log("r:" + run(y));
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "r:1boom3\n", moving_gc);
    }
}
