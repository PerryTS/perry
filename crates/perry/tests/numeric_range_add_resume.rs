//! The numeric-window `arr[i] = arr[i] + delta` kernel: single fused pass
//! with a resume contract, replacing the two-pass all-or-nothing version.
//!
//! `bench_numeric_array_downgrade` spent ~2.9ns per element in
//! `js_array_numeric_range_add` — two full passes over a 1MB window
//! (validate-all, then mutate-all) with a branchy NaN-box decode per slot per
//! pass, against node's 0.64ns. The all-or-nothing contract was stronger than
//! the source semantics require: each element gets exactly one `+ delta`
//! either way, so mutating up to the first non-number and letting the
//! ordinary loop RESUME there (`ret <= -2`, resume index `-ret - 2`) is
//! observably identical and halves the memory traffic. Measured: the
//! benchmark went 18ms -> 4ms, node parity, identical checksum.
//!
//! What these tests pin is the resume contract's OBSERVABLE semantics — the
//! part the benchmark never exercises, because its windows are entirely
//! numeric. A non-number mid-window must produce exactly node's answer:
//! one increment per element, concatenation where `+` concatenates.

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

fn kept_ir(stderr: &str) -> String {
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

/// The tier is reached at all (the helper call is in the IR), and a purely
/// numeric window over a genuinely downgraded array produces node's numbers.
#[test]
fn a_numeric_window_over_a_downgraded_array_takes_the_kernel() {
    let source = r#"
const arr: any[] = [];
for (let i = 0; i < 100; i++) arr.push(i);
arr[50] = { v: 7 };
function bump(a: any[]): void {
  for (let i = 0; i < 50; i++) { a[i] = a[i] + 1; }
}
bump(arr); bump(arr);
console.log("r:" + arr[0] + ":" + arr[49] + ":" + (arr[50] as any).v);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), source);
    assert!(
        kept_ir(&stderr).contains("js_array_numeric_range_add"),
        "the numeric-range-add tier must claim the any[] window loop"
    );
    for moving_gc in [false, true] {
        assert_stdout(&run(&bin, dir.path(), moving_gc), "r:2:51:7\n", moving_gc);
    }
}

/// THE resume contract: a non-number mid-window. Elements before it get their
/// increment from the kernel's partial pass; the element itself and everything
/// after go through the ordinary loop — one `+` each, concatenating exactly
/// where node concatenates.
#[test]
fn a_non_number_mid_window_resumes_the_ordinary_loop_with_node_semantics() {
    let source = r#"
function bump(arr: any[]): void {
  for (let i = 0; i < arr.length; i++) { arr[i] = arr[i] + 1; }
}
const a: any[] = [1, { v: 7 }, "s", 4.5];
bump(a);
console.log(JSON.stringify(a));
const b: any[] = [];
for (let i = 0; i < 100; i++) b.push(i);
b[50] = "mid";
bump(b);
console.log("b49:" + b[49] + " b50:" + b[50] + " b51:" + b[51] + " b99:" + b[99]);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "[2,\"[object Object]1\",\"s1\",5.5]\nb49:50 b50:mid1 b51:52 b99:100\n",
            moving_gc,
        );
    }
}

/// Every element gets EXACTLY one increment — the property the resume encoding
/// must preserve (a double-apply on the mutated prefix is the failure mode of
/// a wrong resume index). NaN elements stay NaN and never corrupt into tags.
#[test]
fn exactly_one_increment_per_element_including_nan_slots() {
    let source = r#"
const arr: any[] = [];
for (let i = 0; i < 64; i++) arr.push(i);
arr[10] = NaN;
arr[40] = "x";
function bump(a: any[]): void {
  for (let i = 0; i < a.length; i++) { a[i] = a[i] + 1; }
}
bump(arr);
console.log("r:" + arr[9] + ":" + (Number.isNaN(arr[10] as number) ? "nan" : "BAD") + ":" + arr[39] + ":" + arr[40] + ":" + arr[63]);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, _) = compile(dir.path(), source);
    for moving_gc in [false, true] {
        assert_stdout(
            &run(&bin, dir.path(), moving_gc),
            "r:10:nan:40:x1:64\n",
            moving_gc,
        );
    }
}
