//! #9379: the packed-f64 loop clone does not need a GC poll, and paid dearly
//! for having one.
//!
//! `stmt/loops.rs` already skips the back-edge poll inside three loop-clone
//! fact scopes, with the reasoning written out there: a poll exists so an
//! ALLOCATING body can defer a collection, `loop_may_allocate` answers from
//! the HIR (where `arr[i] = e` is a generic `IndexSet` that CAN reallocate),
//! and inside a fact scope codegen knows better because the clone is call-free
//! or it is not entered. That comment even anticipates the next clone admitted
//! on the identical argument.
//!
//! The packed-f64 clone IS that clone and was simply not listed. Its entry
//! guard proves a live packed raw-f64 plain Array with the loop window in
//! bounds; reads and writes lower to bare `double` load/store over existing
//! slots, so nothing grows, reallocates, or writes a heap edge; and the
//! matcher admits no calls, closures or awaits into the body.
//!
//! ## Why it cost more than its own instructions
//!
//! The poll's armed word is loaded VOLATILE, which is a clobber inside the
//! loop, so the cached packed receiver base had to be re-derived on every
//! element. That is why striding the poll 1-in-64 (#9316) did not recover the
//! loss while removing it does: the cost was the clobber, not the frequency.
//!
//! Measured on `bench_numeric_array_numeric` (250k x 250, quiet host):
//! 45 -> 38 ms against node's 38. A forced-arm build with polls disabled
//! entirely also lands on 38, so this recovers the whole gap and no more.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

/// The `bench_numeric_array_numeric` shape: a read-modify-write over a
/// `number[]` that the packed-f64 range tier claims.
const SOURCE: &str = r#"
const SIZE = 4096;
const arr: number[] = [];
for (let i = 0; i < SIZE; i++) arr.push(i);
let checksum = 0;
for (let iter = 0; iter < 8; iter++) {
  for (let i = 0; i < arr.length; i++) arr[i] = arr[i] + 1;
  checksum = checksum + arr[0] + arr[arr.length - 1];
}
console.log("checksum:" + checksum);
"#;

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

/// The emitted lines belonging to the packed-f64 fast clone: from its loop
/// condition block to its exit. Everything outside is another tier's code and
/// is none of this test's business.
fn packed_clone_region(ir: &str) -> String {
    let lines: Vec<&str> = ir.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with("for.packed_f64_fast.cond"))
        .unwrap_or_else(|| panic!("no packed-f64 fast clone in the emitted IR"));
    let end = lines[start..]
        .iter()
        .position(|l| l.starts_with("for.packed_f64_fast.exit"))
        .map(|off| start + off)
        .unwrap_or(lines.len());
    lines[start..end].join("\n")
}

fn run(bin: &Path, dir: &Path, gc_stress: bool) -> Output {
    let mut command = Command::new(bin);
    command.current_dir(dir);
    if gc_stress {
        command
            .env("PERRY_GC_HEAP_LIMIT", "8")
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    }
    command.output().expect("run compiled binary")
}

/// The clone's fast block carries no poll, and the program is still correct —
/// including under forced, verified evacuation, which is the arm that matters
/// when a safepoint has been removed.
#[test]
fn packed_f64_clone_emits_no_poll_and_stays_correct() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), SOURCE);
    let ir = kept_ir(&stderr);

    // Vacuity guard first: the absence below is only meaningful while the
    // fixture still ADMITS the packed-f64 clone. If the tier stops claiming
    // this loop, "no poll" would pass for the wrong reason.
    assert!(
        ir.contains("packed_f64_range_store.fast") || ir.contains("for.packed_f64_fast"),
        "fixture no longer admits the packed-f64 loop clone, so the poll \
         assertion below would pass vacuously"
    );

    // Count polls INSIDE the clone only. The module's other loops (the fill
    // loop, the outer iteration loop) are not claimed by this tier and keep
    // their polls legitimately — counting module-wide would assert something
    // this change never claimed.
    let clone_polls = packed_clone_region(&ir)
        .lines()
        .filter(|l| l.contains("PERRY_GC_POLL_ARMED"))
        .count();
    assert_eq!(
        clone_polls, 0,
        "the packed-f64 clone still polls the GC; its volatile armed load is a \
         clobber inside the loop, which forces the cached receiver base to be \
         re-derived per element"
    );

    // Node is the oracle rather than a hand-computed constant. My first draft
    // asserted a value I derived by hand and got wrong; perry was right and the
    // test was the bug. An oracle cannot make that mistake.
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run node");
    assert!(node.status.success(), "node failed on the oracle fixture");
    let expected = String::from_utf8_lossy(&node.stdout).into_owned();

    for stress in [false, true] {
        let out = run(&bin, dir.path(), stress);
        assert!(
            out.status.success(),
            "binary failed (gc_stress={stress})\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected,
            "packed-f64 clone produced the wrong sum (gc_stress={stress})"
        );
    }
}

/// A loop the clone does NOT claim keeps its poll: the skip is scoped to the
/// fact, not applied to loops generally. Here the body calls out, so the
/// matcher refuses it and `loop_may_allocate` is right to demand a safepoint.
#[test]
fn a_calling_loop_still_polls() {
    const CALLING: &str = r#"
const arr: number[] = [];
for (let i = 0; i < 512; i++) arr.push(i);
const box: string[] = [];
for (let i = 0; i < arr.length; i++) {
  arr[i] = arr[i] + 1;
  box.push("g" + (i % 3));      // allocates: the clone must not claim this
  if (box.length > 16) box.length = 0;
}
console.log("n:" + arr[0] + "," + arr[511] + "," + box.length);
"#;
    let dir = tempfile::tempdir().expect("tempdir");
    let (bin, stderr) = compile(dir.path(), CALLING);
    let ir = kept_ir(&stderr);
    assert!(
        ir.contains("PERRY_GC_POLL_ARMED"),
        "an allocating loop body must keep its GC poll — the #9379 skip is \
         scoped to the packed-f64 fact, not a general licence"
    );
    let node = Command::new("node")
        .current_dir(dir.path())
        .arg("--experimental-strip-types")
        .arg(dir.path().join("main.ts"))
        .output()
        .expect("run node");
    assert!(node.status.success(), "node failed on the oracle fixture");
    let out = run(&bin, dir.path(), true);
    assert!(out.status.success(), "binary failed under gc stress");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&node.stdout)
    );
}
